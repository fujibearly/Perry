mod agent_loop;
mod cli;
mod client;
mod config;
mod escalation;
mod function;
#[cfg(feature = "mcp")]
mod mcp;
mod rag;
mod render;
mod repl;
mod safety;
mod serve;
mod skill;
#[macro_use]
mod utils;

#[macro_use]
extern crate log;

use crate::cli::Cli;
use crate::client::{
    call_chat_completions, call_chat_completions_streaming, format_usage_cost,
    format_usage_cost_with, list_models,
    openai_responses::{
        format_openai_responses_live_debug_progress, format_openai_responses_live_progress,
        format_openai_responses_usage_cost, run_openai_responses_multi_agent,
        OpenAIResponsesLiveTraceEvent, OpenAIResponsesOutput, OpenAIResponsesProgress,
    },
    ModelType, TokenUsage,
};
use crate::config::{
    ensure_parent_exists, list_agents, load_env_file, macro_execute, Config, GlobalConfig, Input,
    MultiAgentHostedTool, RoleLike, WorkingMode, CODE_ROLE, EXPLAIN_SHELL_ROLE, SHELL_ROLE,
    TEMP_SESSION_NAME,
};
use crate::render::render_error;
use crate::repl::Repl;
use crate::utils::*;

use anyhow::{bail, Result};
use clap::Parser;
use inquire::Text;
use parking_lot::RwLock;
use simplelog::{format_description, ConfigBuilder, LevelFilter, SimpleLogger, WriteLogger};
use std::{
    env,
    path::Path,
    process,
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::main]
async fn main() -> Result<()> {
    load_env_file()?;
    if get_env_var("START_TIME_MS").is_err() {
        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            let ms = duration.as_millis().to_string();
            crate::utils::set_dual_env_var("START_TIME_MS", &ms);
        }
    }
    let cli = Cli::parse();
    set_spinners_enabled(!cli.no_spinner);
    let text = cli.text()?;
    let working_mode = if cli.serve.is_some() {
        WorkingMode::Serve
    } else if text.is_none() && !cli.has_files() {
        WorkingMode::Repl
    } else {
        WorkingMode::Cmd
    };
    let info_flag = cli.info
        || cli.sync_models
        || {
            #[cfg(feature = "mcp")]
            { cli.sync_mcp }
            #[cfg(not(feature = "mcp"))]
            { false }
        }
        || cli.list_models
        || cli.list_roles
        || cli.list_agents
        || cli.list_rags
        || cli.list_macros
        || cli.list_sessions;
    setup_logger(working_mode.is_serve())?;
    let config = Arc::new(RwLock::new(Config::init(working_mode, info_flag).await?));
    // Remove leftover status files from crashed/killed aichat processes
    crate::agent_loop::cleanup_stale_status_files();
    if let Err(err) = run(config, cli, text).await {
        render_error(err);
        #[cfg(feature = "mcp")]
        mcp::shutdown_all_mcp_servers();
        std::process::exit(1);
    }
    #[cfg(feature = "mcp")]
    mcp::shutdown_all_mcp_servers();
    Ok(())
}

async fn run(config: GlobalConfig, cli: Cli, text: Option<String>) -> Result<()> {
    let abort_signal = create_abort_signal();
    let files = cli.files();

    configure_multi_agent(&config, &cli)?;

    if cli.sync_models {
        let url = config.read().sync_models_url();
        return Config::sync_models(&url, abort_signal.clone()).await;
    }

    #[cfg(feature = "mcp")]
    if cli.sync_mcp {
        let servers = config.read().mcp_servers.clone();
        let cache_dir = Config::mcp_cache_dir();
        match mcp::sync_mcp_tools(&servers, &cache_dir).await {
            Ok(registry) => {
                if registry.is_empty() {
                    println!("No MCP servers configured or all disabled.");
                } else {
                    let mut counts: std::collections::HashMap<&str, usize> =
                        std::collections::HashMap::new();
                    for entry in registry.values() {
                        *counts.entry(&entry.server_name).or_default() += 1;
                    }
                    println!("MCP tools synced:");
                    for (server, count) in &counts {
                        println!("  {server} ({count} tools)");
                    }
                }
            }
            Err(e) => {
                eprintln!("MCP sync failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    if cli.list_models {
        for model in list_models(&config.read(), ModelType::Chat) {
            println!("{}", model.id());
        }
        return Ok(());
    }
    if cli.list_roles {
        let roles = Config::list_roles(true).join("\n");
        println!("{roles}");
        return Ok(());
    }
    if cli.list_agents {
        let agents = list_agents().join("\n");
        println!("{agents}");
        return Ok(());
    }
    if cli.list_rags {
        let rags = Config::list_rags().join("\n");
        println!("{rags}");
        return Ok(());
    }
    if cli.list_macros {
        let macros = Config::list_macros().join("\n");
        println!("{macros}");
        return Ok(());
    }

    validate_multi_agent_mode(&config, &cli)?;

    if cli.dry_run {
        config.write().dry_run = true;
    }
    if cli.show_cost {
        config.write().show_cost = true;
    }
    if cli.show_dialog {
        config.write().agent_loop.show_dialog = true;
    }
    if cli.dialog_no_truncate {
        config.write().agent_loop.dialog_no_truncate = true;
    }
    let show_dialog = config.read().agent_loop.show_dialog;
    crate::utils::set_dual_env_var(
        "AGENT_LOOP_SHOW_DIALOG",
        if show_dialog { "true" } else { "false" },
    );
    let dialog_no_truncate = config.read().agent_loop.dialog_no_truncate;
    crate::utils::set_dual_env_var(
        "AGENT_LOOP_DIALOG_NO_TRUNCATE",
        if dialog_no_truncate { "true" } else { "false" },
    );

    let _dialog_sink_task = if show_dialog {
        let (sink, mut rx) = crate::agent_loop::DialogTraceSink::new();
        config.write().set_dialog_sink(sink);
        Some(tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                crate::agent_loop::render_dialog_event(&event);
            }
        }))
    } else {
        None
    };

    if cli.debug {
        config.write().agent_loop.debug = true;
        crate::utils::set_dual_env_var("AGENT_LOOP_DEBUG", "true");
    }
    if cli.wslinks {
        crate::utils::set_dual_env_var("WSLINKS", "true");
    }
    if let Some(ref autonomy_str) = cli.autonomy {
        if let Some(level) = crate::safety::AutonomyLevel::from_str_loose(autonomy_str) {
            config.write().safety.autonomy = Some(level);
        } else {
            bail!(
                "Invalid autonomy posture '{autonomy_str}'. Valid postures: readonly, consult, reversible, disruptive, destructive"
            );
        }
    }

    if let Some(agent) = &cli.agent {
        let session = cli.session.as_ref().map(|v| match v {
            Some(v) => v.as_str(),
            None => TEMP_SESSION_NAME,
        });
        if !cli.agent_variable.is_empty() {
            config.write().agent_variables = Some(
                cli.agent_variable
                    .chunks(2)
                    .map(|v| (v[0].to_string(), v[1].to_string()))
                    .collect(),
            );
        }

        let ret = Config::use_agent(&config, agent, session, abort_signal.clone()).await;
        config.write().agent_variables = None;
        ret?;
        if config.read().multi_agent.enabled && config.read().session.is_some() {
            bail!("Responses multi-agent mode does not support agent preludes that open a session");
        }
    } else {
        if let Some(prompt) = &cli.prompt {
            config.write().use_prompt(prompt)?;
        } else if let Some(name) = &cli.role {
            config.write().use_role(name)?;
        } else if cli.execute {
            config.write().use_role(SHELL_ROLE)?;
        } else if cli.code {
            config.write().use_role(CODE_ROLE)?;
        }
        if let Some(session) = &cli.session {
            config
                .write()
                .use_session(session.as_ref().map(|v| v.as_str()))?;
        }
        if let Some(rag) = &cli.rag {
            Config::use_rag(&config, Some(rag), abort_signal.clone()).await?;
        }
    }
    if cli.list_sessions {
        let sessions = config.read().list_sessions().join("\n");
        println!("{sessions}");
        return Ok(());
    }
    if let Some(model_id) = &cli.model {
        config.write().set_model(model_id)?;
    }
    if !cli.use_tools.is_empty() {
        let joined_tools = cli.use_tools.join(",");
        config.read().validate_tool_names(&joined_tools)?;
        config.write().set_use_tools(Some(joined_tools));
    }
    if cli.no_stream {
        config.write().stream = false;
    }
    if cli.empty_session {
        config.write().empty_session()?;
    }
    if cli.save_session {
        config.write().set_save_session_this_time()?;
    }
    if cli.info {
        config.write().apply_info_prelude()?;
        let info = config.read().info()?;
        println!("{info}");
        return Ok(());
    }
    if let Some(addr) = cli.serve {
        return serve::run(config, addr).await;
    }
    let is_repl = config.read().working_mode.is_repl();
    if cli.rebuild_rag {
        Config::rebuild_rag(&config, abort_signal.clone()).await?;
        if is_repl {
            return Ok(());
        }
    }
    if let Some(name) = &cli.macro_name {
        macro_execute(&config, name, text.as_deref(), abort_signal.clone()).await?;
        return Ok(());
    }
    let res = if cli.execute && !is_repl {
        let input = create_input(&config, text, &files, abort_signal.clone()).await?;
        shell_execute(&config, &SHELL, input, abort_signal.clone()).await
    } else {
        config.write().apply_prelude()?;
        if config.read().multi_agent.enabled && config.read().session.is_some() {
            bail!("Responses multi-agent mode does not support sessions opened by command preludes");
        }
        match is_repl {
            false => {
                let mut input = create_input(&config, text, &files, abort_signal.clone()).await?;
                input.use_embeddings(abort_signal.clone()).await?;
                start_directive(&config, input, cli.code, abort_signal).await
            }
            true => {
                if !*IS_STDOUT_TERMINAL {
                    bail!("No TTY for REPL")
                }
                start_interactive(&config).await
            }
        }
    };

    if let Some(task) = _dialog_sink_task {
        drop(config.write().dialog_sink.take());
        let _ = tokio::time::timeout(Duration::from_millis(500), task).await;
    }
    res
}

fn configure_multi_agent(config: &GlobalConfig, cli: &Cli) -> Result<()> {
    let enabled = cli.multi_agent || config.read().multi_agent.enabled;
    if cli.max_concurrent_subagents.is_some() && !enabled {
        bail!(
            "--max-concurrent-subagents requires multi-agent mode; enable it with --multi-agent or multi_agent.enabled"
        );
    }
    if cli.show_agent_trace && !enabled {
        bail!(
            "--show-agent-trace requires multi-agent mode; enable it with --multi-agent or multi_agent.enabled"
        );
    }
    for (is_set, option) in [
        (cli.web_search, "--web-search"),
        (cli.max_output_tokens.is_some(), "--max-output-tokens"),
        (cli.service_tier.is_some(), "--service-tier"),
    ] {
        if is_set && !enabled {
            bail!(
                "{option} requires multi-agent mode; enable it with --multi-agent or multi_agent.enabled"
            );
        }
    }

    let mut config = config.write();
    config.multi_agent.enabled = enabled;
    if let Some(max_concurrent_subagents) = cli.max_concurrent_subagents {
        config.multi_agent.max_concurrent_subagents = Some(max_concurrent_subagents);
    }
    if cli.show_agent_trace {
        config.multi_agent.show_trace = true;
    }
    if cli.web_search
        && !config
            .multi_agent
            .hosted_tools
            .iter()
            .any(|tool| matches!(tool, MultiAgentHostedTool::WebSearch { .. }))
    {
        config
            .multi_agent
            .hosted_tools
            .push(MultiAgentHostedTool::web_search());
    }
    if let Some(max_output_tokens) = cli.max_output_tokens {
        config.multi_agent.max_output_tokens = Some(max_output_tokens);
    }
    if let Some(service_tier) = cli.service_tier {
        config.multi_agent.service_tier = service_tier;
    }
    config.multi_agent.validate()
}

fn validate_multi_agent_mode(config: &GlobalConfig, cli: &Cli) -> Result<()> {
    if !config.read().multi_agent.enabled {
        return Ok(());
    }
    if cli.serve.is_some() {
        bail!("Responses multi-agent mode does not support --serve");
    }
    if cli.execute {
        bail!("Responses multi-agent mode does not support --execute");
    }
    if cli.macro_name.is_some() {
        bail!("Responses multi-agent mode does not support macros");
    }
    if cli.session.is_some() {
        bail!("Responses multi-agent mode does not support named or temporary sessions");
    }
    if cli.empty_session || cli.save_session {
        bail!("Responses multi-agent mode does not support session management options");
    }
    if cli.info || cli.list_sessions || cli.rebuild_rag {
        return Ok(());
    }
    if config.read().working_mode.is_repl() {
        bail!("Responses multi-agent mode supports one-shot command input only; REPL is not supported");
    }
    Ok(())
}

async fn start_directive(
    config: &GlobalConfig,
    input: Input,
    code_mode: bool,
    abort_signal: AbortSignal,
) -> Result<()> {
    let model = input.role().model().clone();
    let usage_summary = if config.read().multi_agent.enabled {
        let output = run_multi_agent_directive(config, input, code_mode, abort_signal).await?;
        config.read().show_cost.then(|| {
            format_openai_responses_usage_cost(&model, &output.turns, output.pricing_context)
        })
    } else {
        let (usage, cost) = run_directive(config, input, code_mode, abort_signal).await?;
        config
            .read()
            .show_cost
            .then(|| format_usage_cost_with(&model, usage, (cost > 0.0).then_some(cost)))
    };
    if let Some(summary) = usage_summary {
        eprintln!("{summary}");
    }
    config.write().exit_session()?;
    Ok(())
}

async fn run_directive(
    config: &GlobalConfig,
    input: Input,
    code_mode: bool,
    abort_signal: AbortSignal,
) -> Result<(TokenUsage, f64)> {
    let (progress, event_rx) = crate::agent_loop::AgentLoopProgress::live();
    let params = crate::agent_loop::AgentLoopParams {
        config,
        abort_signal: abort_signal.clone(),
        code_mode,
        progress: progress.clone(),
    };

    let agent_loop_config = config.read().agent_loop.clone();
    let agent_label = config.read().agent.as_ref()
        .map(|a| a.name().to_string())
        .or_else(|| config.read().role.as_ref().map(|r| r.name().to_string()))
        .or_else(|| {
            get_env_var("AGENT_NAME")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            get_env_var("INVOKING_AGENT")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|inv| format!("nano-{inv}"))
        })
        .unwrap_or_else(|| "perry".to_string());

    // Sub-agents (depth > 0) should not overwrite the pane title — only the root owns it.
    let current_depth: usize = get_env_var("AGENT_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut agent_loop_config = agent_loop_config;
    if current_depth > 0 {
        agent_loop_config.osc_title = false;
        agent_loop_config.notify = false;
    } else if get_env_var("AGENT_COLOR").is_err() {
        crate::utils::set_dual_env_var("AGENT_COLOR", crate::agent_loop::AGENT_PALETTE[0].0);
    }

    // If no trace/observability needed and stdout is not a terminal, run without rendering overhead
    if !agent_loop_config.show_trace
        && !agent_loop_config.show_dialog
        && !agent_loop_config.osc_title
        && !agent_loop_config.status_file
        && !agent_loop_config.notify
        && !*IS_STDOUT_TERMINAL
    {
        drop(event_rx);
        let output = crate::agent_loop::run(input, params).await?;
        return Ok((output.usage, output.cost));
    }

    // Run with observability rendering
    let (spinner, spinner_rx) = Spinner::create("");
    let loop_future = crate::agent_loop::run(input, params);
    let live_run = async {
        tokio::pin!(loop_future);
        let mut event_rx = event_rx;
        // Conservative heartbeat — 2s for spinner message updates (avoids CPU churn)
        let mut heartbeat = tokio::time::interval(Duration::from_secs(2));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut trace_header_printed = false;

        loop {
            tokio::select! {
                result = &mut loop_future => {
                    // Drain remaining events
                    while let Ok(event) = event_rx.try_recv() {
                        let snapshot = progress.snapshot();
                        let _ = crate::agent_loop::render_event(
                            &event, &snapshot, &agent_loop_config, &agent_label,
                            &spinner, &mut trace_header_printed,
                        );
                    }
                    break result;
                }
                Some(event) = event_rx.recv() => {
                    let snapshot = progress.snapshot();
                    let _ = crate::agent_loop::render_event(
                        &event, &snapshot, &agent_loop_config, &agent_label,
                        &spinner, &mut trace_header_printed,
                    );
                    // Update spinner message on events (lightweight, no animation tick)
                    if *IS_STDOUT_TERMINAL {
                        let msg = crate::agent_loop::format_spinner_message(&snapshot);
                        let _ = spinner.set_message(msg);
                    }
                }
                _ = heartbeat.tick() => {
                    // Periodic spinner + title update for long-running tools
                    let snapshot = progress.snapshot();
                    if *IS_STDOUT_TERMINAL {
                        let msg = crate::agent_loop::format_spinner_message(&snapshot);
                        let _ = spinner.set_message(msg);
                    }
                    if agent_loop_config.osc_title {
                        let title = crate::agent_loop::format_heartbeat_title(&snapshot, &agent_label);
                        crate::agent_loop::update_terminal_title(&title);
                    }
                }
            }
        }
    };
    let result =
        abortable_run_with_spinner_rx(live_run, spinner_rx, abort_signal.clone()).await;

    // Cleanup status file on exit
    if agent_loop_config.status_file {
        crate::agent_loop::cleanup_status_file();
    }
    // Reset terminal title — keep "done" visible, don't overwrite with "idle"
    // The title stays until the next command runs in the pane.
    if agent_loop_config.osc_title {
        let pid = std::process::id();
        crate::agent_loop::update_terminal_title(&format!("done | {agent_label}:{pid}"));
    }

    let output = result?;
    Ok((output.usage, output.cost))
}

async fn run_multi_agent_directive(
    config: &GlobalConfig,
    input: Input,
    code_mode: bool,
    abort_signal: AbortSignal,
) -> Result<OpenAIResponsesOutput> {
    let model = input.role().model().clone();
    let (progress, mut live_trace_rx) = OpenAIResponsesProgress::live();
    let extract_code = !*IS_STDOUT_TERMINAL && code_mode;
    let show_trace = config.read().multi_agent.show_trace;
    let debug_logging = log::log_enabled!(log::Level::Debug);
    let debug_logs_to_stderr = debug_logging && logger_targets_stderr();
    config.write().before_chat_completion(&input)?;
    let (spinner, spinner_rx) = Spinner::create(if debug_logs_to_stderr || !*IS_STDOUT_TERMINAL {
        ""
    } else {
        "Generating"
    });
    let request_progress = progress.clone();
    let request =
        run_openai_responses_multi_agent(config, &input, abort_signal.clone(), request_progress);
    let live_run = async {
        tokio::pin!(request);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_debug_heartbeat = None;
        let mut trace_header_printed = false;
        let mut render_trace = |event: OpenAIResponsesLiveTraceEvent| -> Result<()> {
            if !show_trace {
                return Ok(());
            }
            let line = if trace_header_printed {
                format!("  {}", event.line())
            } else {
                trace_header_printed = true;
                format!("Agent trace (live):\n  {}", event.line())
            };
            if *IS_STDOUT_TERMINAL {
                spinner.print_line(line)?;
            } else {
                eprintln!("{line}");
            }
            Ok(())
        };

        loop {
            tokio::select! {
                result = &mut request => {
                    while let Ok(event) = live_trace_rx.try_recv() {
                        render_trace(event)?;
                    }
                    break result;
                }
                Some(event) = live_trace_rx.recv() => {
                    render_trace(event)?;
                }
                _ = heartbeat.tick() => {
                    let now = Instant::now();
                    let snapshot = progress.live_snapshot();
                    let message = format_openai_responses_live_progress(&snapshot, now);
                    if !debug_logs_to_stderr && *IS_STDOUT_TERMINAL {
                        spinner.set_message(message.clone())?;
                    }
                    let should_log_heartbeat = match last_debug_heartbeat {
                        Some(last) => now.duration_since(last) >= Duration::from_secs(10),
                        None => true,
                    };
                    if should_log_heartbeat {
                        let debug_message =
                            format_openai_responses_live_debug_progress(&snapshot, now);
                        debug!("OpenAI Responses heartbeat: {debug_message}");
                        last_debug_heartbeat = Some(now);
                    }
                }
            }
        }
    };
    let result = abortable_run_with_spinner_rx(live_run, spinner_rx, abort_signal.clone()).await;
    let mut output = match result {
        Ok(output) => output,
        Err(error) => {
            let (turns, pricing_context) = progress.snapshot();
            if !turns.is_empty() && config.read().show_cost {
                eprintln!(
                    "Partial Responses usage before failure:\n{}",
                    format_openai_responses_usage_cost(&model, &turns, pricing_context)
                );
            }
            return Err(error);
        }
    };
    if extract_code {
        output.text = extract_code_block(&strip_think_tag(&output.text)).to_string();
    }
    if !output.text.is_empty() {
        config.read().print_markdown(&output.text)?;
    }
    config
        .write()
        .after_chat_completion(&input, &output.text, &[])?;
    Ok(output)
}

fn logger_targets_stderr() -> bool {
    let Ok((_, log_path)) = Config::log_config(false) else {
        return false;
    };
    match log_path.as_deref() {
        None => true,
        Some(path) => ["/dev/stderr", "/dev/fd/2", "/proc/self/fd/2"]
            .iter()
            .any(|candidate| path == Path::new(candidate)),
    }
}

async fn start_interactive(config: &GlobalConfig) -> Result<()> {
    let mut repl: Repl = Repl::init(config)?;
    repl.run().await
}

#[async_recursion::async_recursion]
async fn shell_execute(
    config: &GlobalConfig,
    shell: &Shell,
    mut input: Input,
    abort_signal: AbortSignal,
) -> Result<()> {
    let client = input.create_client()?;
    let show_dialog = config.read().agent_loop.show_dialog;
    let no_truncate = config.read().agent_loop.dialog_no_truncate;
    let dialog_sink = config.read().dialog_sink();
    let model_id = client.model().id().to_string();

    if show_dialog {
        let prompt_content = match input.build_messages() {
            Ok(msgs) => crate::agent_loop::format_messages_dialog(&msgs, no_truncate),
            Err(_) => input.text().to_string(),
        };
        if let Some(sink) = &dialog_sink {
            sink.emit(crate::agent_loop::dialog_trace::DialogEvent {
                sequence: 0,
                trace_id: "shell-execute".into(),
                source: crate::agent_loop::dialog_trace::DialogSource::ShellExecute,
                agent: "shell".into(),
                configured_model: model_id.clone(),
                wire_model: None,
                pid: std::process::id(),
                turn: 1,
                max_turns: 1,
                direction: crate::agent_loop::DialogDirection::Request,
                content: prompt_content,
            });
        }
    }

    config.write().before_chat_completion(&input)?;
    let (output, _) =
        call_chat_completions(&input, false, true, client.as_ref(), abort_signal.clone()).await?;
    let usage = output.usage();
    let eval_str = output.text;

    if show_dialog {
        if let Some(sink) = &dialog_sink {
            sink.emit(crate::agent_loop::dialog_trace::DialogEvent {
                sequence: 0,
                trace_id: "shell-execute".into(),
                source: crate::agent_loop::dialog_trace::DialogSource::ShellExecute,
                agent: "shell".into(),
                configured_model: model_id,
                wire_model: None,
                pid: std::process::id(),
                turn: 1,
                max_turns: 1,
                direction: crate::agent_loop::DialogDirection::Response,
                content: eval_str.clone(),
            });
        }
    }

    config
        .write()
        .after_chat_completion(&input, &eval_str, &[])?;
    let usage_summary = config
        .read()
        .show_cost
        .then(|| format_usage_cost(client.model(), usage));
    if eval_str.is_empty() {
        bail!("No command generated");
    }
    if config.read().dry_run {
        config.read().print_markdown(&eval_str)?;
        if let Some(summary) = usage_summary {
            eprintln!("{summary}");
        }
        return Ok(());
    }
    if *IS_STDOUT_TERMINAL {
        let options = ["execute", "revise", "describe", "copy", "quit"];
        let command = warning_text(eval_str.trim());
        let first_letter_color = nu_ansi_term::Color::Cyan;
        let prompt_text = options
            .iter()
            .map(|v| format!("{}{}", color_text(&v[0..1], first_letter_color), &v[1..]))
            .collect::<Vec<String>>()
            .join(&dimmed_text(" | "));
        let mut usage_summary = usage_summary;
        loop {
            println!("{command}");
            if let Some(summary) = usage_summary.take() {
                eprintln!("{summary}");
            }
            let answer_char =
                read_single_key(&['e', 'r', 'd', 'c', 'q'], 'e', &format!("{prompt_text}: "))?;

            match answer_char {
                'e' => {
                    debug!("{} {:?}", shell.cmd, [&shell.arg, &eval_str]);
                    let code = run_command(&shell.cmd, &[&shell.arg, &eval_str], None)?;
                    if code == 0 && config.read().save_shell_history {
                        let _ = append_to_shell_history(&shell.name, &eval_str, code);
                    }
                    process::exit(code);
                }
                'r' => {
                    let revision = Text::new("Enter your revision:").prompt()?;
                    let text = format!("{}\n{revision}", input.text());
                    input.set_text(text);
                    return shell_execute(config, shell, input, abort_signal.clone()).await;
                }
                'd' => {
                    let role = config.read().retrieve_role(EXPLAIN_SHELL_ROLE)?;
                    let explain_model = role
                        .model_id()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| client.model().id());
                    let input = Input::from_str(config, &eval_str, Some(role));
                    if show_dialog {
                        let prompt_content = match input.build_messages() {
                            Ok(msgs) => crate::agent_loop::format_messages_dialog(&msgs, no_truncate),
                            Err(_) => eval_str.clone(),
                        };
                        if let Some(sink) = &dialog_sink {
                            sink.emit(crate::agent_loop::dialog_trace::DialogEvent {
                                sequence: 0,
                                trace_id: "explain-shell".into(),
                                source: crate::agent_loop::dialog_trace::DialogSource::ShellExecute,
                                agent: "explain-shell".into(),
                                configured_model: explain_model.clone(),
                                wire_model: None,
                                pid: std::process::id(),
                                turn: 1,
                                max_turns: 1,
                                direction: crate::agent_loop::DialogDirection::Request,
                                content: prompt_content,
                            });
                        }
                    }
                    let (description, _) = if input.stream() {
                        call_chat_completions_streaming(
                            &input,
                            client.as_ref(),
                            abort_signal.clone(),
                        )
                        .await?
                    } else {
                        call_chat_completions(
                            &input,
                            true,
                            false,
                            client.as_ref(),
                            abort_signal.clone(),
                        )
                        .await?
                    };
                    if show_dialog {
                        if let Some(sink) = &dialog_sink {
                            sink.emit(crate::agent_loop::dialog_trace::DialogEvent {
                                sequence: 0,
                                trace_id: "explain-shell".into(),
                                source: crate::agent_loop::dialog_trace::DialogSource::ShellExecute,
                                agent: "explain-shell".into(),
                                configured_model: explain_model,
                                wire_model: None,
                                pid: std::process::id(),
                                turn: 1,
                                max_turns: 1,
                                direction: crate::agent_loop::DialogDirection::Response,
                                content: description.text.clone(),
                            });
                        }
                    }
                    if config.read().show_cost {
                        eprintln!("{}", format_usage_cost(client.model(), description.usage()));
                    }
                    println!();
                    continue;
                }
                'c' => {
                    set_text(&eval_str)?;
                    println!("{}", dimmed_text("✓ Copied the command."));
                }
                _ => {}
            }
            break;
        }
    } else {
        println!("{eval_str}");
        if let Some(summary) = usage_summary {
            eprintln!("{summary}");
        }
    }
    Ok(())
}

async fn create_input(
    config: &GlobalConfig,
    text: Option<String>,
    file: &[String],
    abort_signal: AbortSignal,
) -> Result<Input> {
    let input = if file.is_empty() {
        Input::from_str(config, &text.unwrap_or_default(), None)
    } else {
        Input::from_files_with_spinner(
            config,
            &text.unwrap_or_default(),
            file.to_vec(),
            None,
            abort_signal,
        )
        .await?
    };
    if input.is_empty() {
        bail!("No input");
    }
    Ok(input)
}

fn setup_logger(is_serve: bool) -> Result<()> {
    let (log_level, log_path) = Config::log_config(is_serve)?;
    if log_level == LevelFilter::Off {
        return Ok(());
    }
    let crate_name = env!("CARGO_CRATE_NAME");
    let log_filter = match get_env_var("log_filter") {
        Ok(v) => v,
        Err(_) => match is_serve {
            true => format!("{crate_name}::serve"),
            false => crate_name.into(),
        },
    };
    let config = ConfigBuilder::new()
        .add_filter_allow(log_filter)
        .set_time_format_custom(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .set_thread_level(LevelFilter::Off)
        .build();
    match log_path {
        None => {
            SimpleLogger::init(log_level, config)?;
        }
        Some(log_path) => {
            ensure_parent_exists(&log_path)?;
            let log_file = std::fs::File::create(log_path)?;
            WriteLogger::init(log_level, config, log_file)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HostedWebSearchConfig, OpenAIServiceTier, WebSearchContextSize};
    use std::num::NonZeroUsize;

    #[tokio::test]
    async fn list_command_returns_before_info_prelude() {
        let config = Arc::new(RwLock::new(Config {
            cmd_prelude: Some(format!("role:{CODE_ROLE}")),
            ..Default::default()
        }));
        let cli = Cli::try_parse_from(["aichat", "--list-models", "--info"]).unwrap();

        run(config.clone(), cli, None).await.unwrap();

        assert!(config.read().state().is_empty());
    }

    #[test]
    fn cli_enables_multi_agent_and_sets_concurrency_limit() {
        let config = Arc::new(RwLock::new(Config::default()));
        let cli = Cli::try_parse_from([
            "aichat",
            "--multi-agent",
            "--max-concurrent-subagents",
            "4",
            "prompt",
        ])
        .unwrap();

        configure_multi_agent(&config, &cli).unwrap();

        let config = config.read();
        assert!(config.multi_agent.enabled);
        assert_eq!(
            config
                .multi_agent
                .max_concurrent_subagents
                .map(|value| value.get()),
            Some(4)
        );
    }

    #[test]
    fn cli_concurrency_limit_overrides_enabled_config() {
        let config = Arc::new(RwLock::new(Config::default()));
        config.write().multi_agent.enabled = true;
        let cli =
            Cli::try_parse_from(["aichat", "--max-concurrent-subagents", "6", "prompt"]).unwrap();

        configure_multi_agent(&config, &cli).unwrap();

        assert_eq!(
            config
                .read()
                .multi_agent
                .max_concurrent_subagents
                .map(|value| value.get()),
            Some(6)
        );
    }

    #[test]
    fn cli_enables_web_search_and_request_controls() {
        let config = Arc::new(RwLock::new(Config::default()));
        let cli = Cli::try_parse_from([
            "aichat",
            "--multi-agent",
            "--web-search",
            "--max-output-tokens",
            "16000",
            "--service-tier",
            "default",
            "prompt",
        ])
        .unwrap();

        configure_multi_agent(&config, &cli).unwrap();

        let config = config.read();
        assert!(config.multi_agent.enabled);
        assert_eq!(config.multi_agent.hosted_tools.len(), 1);
        assert_eq!(
            config.multi_agent.max_output_tokens.map(NonZeroUsize::get),
            Some(16_000)
        );
        assert_eq!(config.multi_agent.service_tier, OpenAIServiceTier::Default);
    }

    #[test]
    fn cli_multi_agent_preserves_configured_web_search_without_duplication() {
        let config = Arc::new(RwLock::new(Config::default()));
        config.write().multi_agent.hosted_tools = vec![MultiAgentHostedTool::WebSearch {
            config: HostedWebSearchConfig {
                search_context_size: WebSearchContextSize::High,
                ..Default::default()
            },
        }];
        let cli =
            Cli::try_parse_from(["aichat", "--multi-agent", "--web-search", "prompt"]).unwrap();

        configure_multi_agent(&config, &cli).unwrap();

        let config = config.read();
        assert!(config.multi_agent.enabled);
        assert_eq!(config.multi_agent.hosted_tools.len(), 1);
        let MultiAgentHostedTool::WebSearch { config } = &config.multi_agent.hosted_tools[0];
        assert_eq!(config.search_context_size, WebSearchContextSize::High);
    }

    #[test]
    fn concurrency_limit_requires_multi_agent_mode() {
        let config = Arc::new(RwLock::new(Config::default()));
        let cli =
            Cli::try_parse_from(["aichat", "--max-concurrent-subagents", "2", "prompt"]).unwrap();

        let error = configure_multi_agent(&config, &cli).unwrap_err();

        assert!(error
            .to_string()
            .contains("--max-concurrent-subagents requires multi-agent mode"));
    }

    #[test]
    fn agent_trace_flag_requires_multi_agent_mode() {
        let config = Arc::new(RwLock::new(Config::default()));
        let cli = Cli::try_parse_from(["aichat", "--show-agent-trace", "prompt"]).unwrap();

        let error = configure_multi_agent(&config, &cli).unwrap_err();

        assert!(error
            .to_string()
            .contains("--show-agent-trace requires multi-agent mode"));
    }

    #[test]
    fn hosted_request_controls_require_multi_agent_mode() {
        for args in [
            vec!["aichat", "--web-search", "prompt"],
            vec!["aichat", "--max-output-tokens", "1024", "prompt"],
            vec!["aichat", "--service-tier", "flex", "prompt"],
        ] {
            let config = Arc::new(RwLock::new(Config::default()));
            let cli = Cli::try_parse_from(args).unwrap();

            assert!(configure_multi_agent(&config, &cli)
                .unwrap_err()
                .to_string()
                .contains("requires multi-agent mode"));
        }
    }

    #[test]
    fn agent_trace_flag_overrides_enabled_config() {
        let config = Arc::new(RwLock::new(Config::default()));
        config.write().multi_agent.enabled = true;
        let cli = Cli::try_parse_from(["aichat", "--show-agent-trace", "prompt"]).unwrap();

        configure_multi_agent(&config, &cli).unwrap();

        assert!(config.read().multi_agent.show_trace);
    }

    #[test]
    fn multi_agent_accepts_one_shot_command_mode() {
        let config = Arc::new(RwLock::new(Config::default()));
        config.write().multi_agent.enabled = true;
        let cli = Cli::try_parse_from(["aichat", "prompt"]).unwrap();

        validate_multi_agent_mode(&config, &cli).unwrap();
    }

    #[test]
    fn disabled_multi_agent_does_not_restrict_existing_modes() {
        let config = Arc::new(RwLock::new(Config::default()));
        let cli = Cli::try_parse_from(["aichat", "--execute", "prompt"]).unwrap();

        validate_multi_agent_mode(&config, &cli).unwrap();
    }

    #[tokio::test]
    async fn multi_agent_allows_non_generation_info_commands_in_repl_mode() {
        for flag in ["--info", "--list-sessions"] {
            let config = Arc::new(RwLock::new(Config::default()));
            {
                let mut config = config.write();
                config.multi_agent.enabled = true;
                config.working_mode = WorkingMode::Repl;
            }
            let cli = Cli::try_parse_from(["aichat", flag]).unwrap();

            run(config.clone(), cli, None).await.unwrap();

            assert!(config.read().multi_agent.enabled);
        }
    }

    #[tokio::test]
    async fn multi_agent_rebuild_rag_reaches_maintenance_path_in_repl_mode() {
        let config = Arc::new(RwLock::new(Config::default()));
        {
            let mut config = config.write();
            config.multi_agent.enabled = true;
            config.working_mode = WorkingMode::Repl;
        }
        let cli = Cli::try_parse_from(["aichat", "--rebuild-rag"]).unwrap();

        let error = run(config.clone(), cli, None).await.unwrap_err();

        assert_eq!(error.to_string(), "No RAG");
        assert!(config.read().multi_agent.enabled);
    }

    #[test]
    fn multi_agent_info_commands_still_reject_sessions() {
        let config = Arc::new(RwLock::new(Config::default()));
        config.write().multi_agent.enabled = true;

        for flag in ["--info", "--list-sessions", "--rebuild-rag"] {
            let cli = Cli::try_parse_from(["aichat", flag, "--session", "saved"]).unwrap();
            assert!(validate_multi_agent_mode(&config, &cli)
                .unwrap_err()
                .to_string()
                .contains("sessions"));
        }
    }

    #[test]
    fn multi_agent_rejects_incompatible_modes() {
        let config = Arc::new(RwLock::new(Config::default()));
        config.write().multi_agent.enabled = true;

        let serve = Cli::try_parse_from(["aichat", "--serve"]).unwrap();
        assert!(validate_multi_agent_mode(&config, &serve)
            .unwrap_err()
            .to_string()
            .contains("--serve"));

        let execute = Cli::try_parse_from(["aichat", "--execute", "prompt"]).unwrap();
        assert!(validate_multi_agent_mode(&config, &execute)
            .unwrap_err()
            .to_string()
            .contains("--execute"));

        let macro_cli = Cli::try_parse_from(["aichat", "--macro", "example"]).unwrap();
        assert!(validate_multi_agent_mode(&config, &macro_cli)
            .unwrap_err()
            .to_string()
            .contains("macros"));

        let session = Cli::try_parse_from(["aichat", "--session", "saved", "prompt"]).unwrap();
        assert!(validate_multi_agent_mode(&config, &session)
            .unwrap_err()
            .to_string()
            .contains("sessions"));

        config.write().working_mode = WorkingMode::Repl;
        let repl = Cli::try_parse_from(["aichat"]).unwrap();
        assert!(validate_multi_agent_mode(&config, &repl)
            .unwrap_err()
            .to_string()
            .contains("REPL"));
    }
}
