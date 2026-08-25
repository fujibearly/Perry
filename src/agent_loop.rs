//! Client-side agent loop: iterative tool-call execution for any LLM provider.
//!
//! This module provides a unified, provider-agnostic agent loop that replaces the
//! recursive `run_directive` / `ask_inner` pattern. It supports parallel tool execution,
//! configurable turn budgets, sub-agent subprocess delegation, live progress reporting,
//! external observability signals (OSC titles, status files, notifications), and a
//! built-in planning tool.

use crate::client::{
    call_chat_completions_raw, call_chat_completions_streaming_raw, ChatCompletionsOutput,
    TokenUsage,
};
use crate::config::{GlobalConfig, Input};
use crate::function::{FunctionDeclaration, JsonSchema, ToolCall, ToolResult};
use crate::utils::*;

use anyhow::{bail, Result};
use futures_util::future::join_all;
use indexmap::IndexMap;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Parameters for a single agent loop invocation.
pub struct AgentLoopParams<'a> {
    pub config: &'a GlobalConfig,
    pub abort_signal: AbortSignal,
    pub code_mode: bool,
    /// Progress reporter (shared with caller for rendering).
    pub progress: AgentLoopProgress,
}

/// Return value from a completed agent loop.
pub struct AgentLoopOutput {
    pub usage: TokenUsage,
    pub final_text: String,
}

/// Events emitted during the agent loop for observability.
#[derive(Debug, Clone)]
pub enum AgentLoopEvent {
    TurnStart {
        turn: usize,
        max_turns: usize,
    },
    ToolStart {
        name: String,
        id: Option<String>,
    },
    ToolComplete {
        name: String,
        duration: Duration,
        success: bool,
    },
    SubAgentStart {
        agent_name: String,
        pid: u32,
    },
    SubAgentComplete {
        agent_name: String,
        pid: u32,
        duration: Duration,
        success: bool,
    },
    PlanReceived {
        content: String,
    },
    BudgetWarning {
        turn: usize,
        max_turns: usize,
    },
    BudgetExhausted {
        max_turns: usize,
    },
    LoopComplete,
}

/// Thread-safe progress tracker for the agent loop.
///
/// Mirrors the `OpenAIResponsesProgress` pattern: events are pushed into an
/// unbounded channel and consumed by a rendering loop in the caller.
#[derive(Debug, Clone, Default)]
pub struct AgentLoopProgress {
    state: Arc<Mutex<AgentLoopProgressState>>,
}

#[derive(Debug, Default)]
struct AgentLoopProgressState {
    started_at: Option<Instant>,
    current_turn: usize,
    max_turns: usize,
    active_tools: Vec<String>,
    event_sender: Option<UnboundedSender<AgentLoopEvent>>,
}

/// A point-in-time snapshot of agent loop progress for spinner/heartbeat rendering.
#[derive(Debug, Clone)]
pub struct AgentLoopSnapshot {
    pub current_turn: usize,
    pub max_turns: usize,
    pub active_tools: Vec<String>,
    pub elapsed: Duration,
}

impl AgentLoopProgress {
    /// Create a progress tracker with a live event channel.
    pub fn live() -> (Self, UnboundedReceiver<AgentLoopEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let progress = Self::default();
        {
            let mut state = progress.state.lock();
            state.started_at = Some(Instant::now());
            state.event_sender = Some(sender);
        }
        (progress, receiver)
    }

    /// Emit an event (fire-and-forget; does nothing if no receiver is attached).
    pub fn emit(&self, event: AgentLoopEvent) {
        let sender = self.state.lock().event_sender.clone();
        if let Some(sender) = sender {
            let _ = sender.send(event);
        }
    }

    /// Get a point-in-time snapshot for spinner rendering.
    pub fn snapshot(&self) -> AgentLoopSnapshot {
        let state = self.state.lock();
        let elapsed = state
            .started_at
            .map(|s| s.elapsed())
            .unwrap_or_default();
        AgentLoopSnapshot {
            current_turn: state.current_turn,
            max_turns: state.max_turns,
            active_tools: state.active_tools.clone(),
            elapsed,
        }
    }

    /// Update the current turn counter (called by the loop on each iteration).
    pub fn set_turn(&self, turn: usize, max_turns: usize) {
        let mut state = self.state.lock();
        state.current_turn = turn;
        state.max_turns = max_turns;
    }

    /// Track an active tool (called when a tool starts executing).
    pub fn add_active_tool(&self, name: &str) {
        self.state.lock().active_tools.push(name.to_string());
    }

    /// Remove a completed tool from the active set.
    pub fn remove_active_tool(&self, name: &str) {
        self.state.lock().active_tools.retain(|n| n != name);
    }
}

// ---------------------------------------------------------------------------
// Parallel tool execution
// ---------------------------------------------------------------------------

/// Execute tool calls concurrently with a bounded semaphore.
///
/// Each tool is dispatched asynchronously (MCP via await, shell via spawn_blocking).
/// Results are returned in the same order as the input calls. Individual tool failures
/// do not cancel siblings — they produce error JSON results.
pub async fn eval_tool_calls_parallel(
    config: &GlobalConfig,
    calls: Vec<ToolCall>,
    abort_signal: AbortSignal,
    progress: &AgentLoopProgress,
) -> Result<Vec<ToolResult>> {
    if calls.is_empty() {
        return Ok(vec![]);
    }

    let calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.");
    }

    let max_concurrency = config.read().agent_loop.max_concurrency;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrency));

    let futures: Vec<_> = calls
        .into_iter()
        .map(|call| {
            let config = config.clone();
            let semaphore = semaphore.clone();
            let _abort_signal = abort_signal.clone();
            let progress = progress.clone();
            async move {
                let _permit = semaphore.acquire().await.unwrap();
                let start = Instant::now();
                progress.emit(AgentLoopEvent::ToolStart {
                    name: call.name.clone(),
                    id: call.id.clone(),
                });
                progress.add_active_tool(&call.name);

                let result = eval_single_tool(&config, &call).await;
                let duration = start.elapsed();

                let output = match result {
                    Ok(value) => {
                        progress.emit(AgentLoopEvent::ToolComplete {
                            name: call.name.clone(),
                            duration,
                            success: true,
                        });
                        if value.is_null() { json!("DONE") } else { value }
                    }
                    Err(_) => {
                        progress.emit(AgentLoopEvent::ToolComplete {
                            name: call.name.clone(),
                            duration,
                            success: false,
                        });
                        json!({
                            "error": {
                                "type": "tool_execution_error",
                                "message": "The tool call failed. Fix its arguments or choose another tool."
                            }
                        })
                    }
                };
                progress.remove_active_tool(&call.name);
                ToolResult::new(call, output)
            }
        })
        .collect();

    let results = join_all(futures).await;

    // Preserve existing behavior: if all results are "DONE" (null tools), return empty
    let is_all_done = results.iter().all(|r| r.output == json!("DONE"));
    if is_all_done {
        return Ok(vec![]);
    }

    Ok(results)
}

/// Dispatch a single tool call asynchronously.
async fn eval_single_tool(config: &GlobalConfig, call: &ToolCall) -> Result<serde_json::Value> {
    // Route 1: MCP tools (async native)
    #[cfg(feature = "mcp")]
    {
        let mcp_call_info = {
            let config_read = config.read();
            config_read.mcp_tools.get(&call.name).map(|entry| {
                let server_name = entry.server_name.clone();
                let original_name = entry.original_name.clone();
                let server_config = config_read
                    .mcp_servers
                    .iter()
                    .find(|s| s.name == server_name)
                    .cloned();
                (server_name, original_name, server_config)
            })
        }; // config_read dropped here, before any await

        if let Some((server_name, original_name, server_config)) = mcp_call_info {
            let server_config = match server_config {
                Some(c) => c,
                None => bail!(
                    "MCP server config '{}' not found for tool '{}'",
                    server_name,
                    call.name
                ),
            };

            let arguments = if call.arguments.is_object() {
                call.arguments.clone()
            } else if let Some(args_str) = call.arguments.as_str() {
                serde_json::from_str(args_str).unwrap_or_else(|_| call.arguments.clone())
            } else {
                call.arguments.clone()
            };

            let timeout = std::time::Duration::from_secs(server_config.timeout);
            return crate::mcp::call_mcp_tool_async(
                &server_config,
                &original_name,
                arguments,
                timeout,
            )
            .await;
        }
    }

    // Route 2: Agent tools (subprocess delegation)
    {
        let is_agent = {
            let config_read = config.read();
            if let Some(agent) = &config_read.agent {
                agent
                    .functions()
                    .find(&call.name)
                    .map_or(false, |f| f.agent)
            } else {
                config_read
                    .functions
                    .find(&call.name)
                    .map_or(false, |f| f.agent)
            }
        };
        if is_agent {
            return eval_agent_tool_subprocess(config, call).await;
        }
    }

    // Route 3: Shell-exec tools (wrapped in spawn_blocking)
    let config = config.clone();
    let call = call.clone();
    tokio::task::spawn_blocking(move || call.eval_shell(&config))
        .await
        .map_err(|e| anyhow::anyhow!("Tool task panicked: {e}"))?
}

/// Spawn a sub-agent as a separate aichat process.
///
/// The sub-agent runs with its own PID, session, turn budget, and observability.
/// Depth is tracked via AICHAT_AGENT_DEPTH env var to prevent infinite nesting.
async fn eval_agent_tool_subprocess(
    config: &GlobalConfig,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    // 1. Check depth limit
    let current_depth: usize = std::env::var("AICHAT_AGENT_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let max_depth = config.read().agent_loop.max_agent_depth;
    if current_depth >= max_depth {
        bail!(
            "Sub-agent nesting depth {} would exceed maximum {}. \
             Increase with `agent_loop.max_agent_depth` in config.",
            current_depth + 1,
            max_depth
        );
    }

    // 2. Resolve agent name — the tool name IS the agent name for agent-flagged tools
    let agent_name = call.name.clone();

    // 3. Build the task message from arguments
    let task_message = if let Some(s) = call.arguments.as_str() {
        s.to_string()
    } else if let Some(obj) = call.arguments.as_object() {
        // Try common argument patterns: "prompt", "task", "message", "input"
        obj.get("prompt")
            .or_else(|| obj.get("task"))
            .or_else(|| obj.get("message"))
            .or_else(|| obj.get("input"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| call.arguments.to_string())
    } else {
        call.arguments.to_string()
    };

    // 4. Spawn the subprocess
    let aichat_bin = std::env::current_exe()?;
    let mut cmd = tokio::process::Command::new(&aichat_bin);
    cmd.arg("--agent").arg(&agent_name);
    cmd.arg(&task_message);

    // Pass depth to child
    cmd.env("AICHAT_AGENT_DEPTH", (current_depth + 1).to_string());

    // Inherit config dir so sub-agent sees same agents/tools/MCP
    if let Ok(config_dir) = std::env::var("AICHAT_CONFIG_DIR") {
        cmd.env("AICHAT_CONFIG_DIR", config_dir);
    }

    // Capture stdout/stderr
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // 5. Run and wait
    let child = cmd.spawn()?;
    let _pid = child.id().unwrap_or(0);
    let output = child.wait_with_output().await?;

    // 6. Return result
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(json!({"output": text}))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!(
                "Sub-agent '{}' exited with code {:?}",
                agent_name,
                output.status.code()
            )
        } else {
            stderr
        };
        Ok(json!({
            "error": {
                "type": "agent_error",
                "message": message
            }
        }))
    }
}

// ---------------------------------------------------------------------------
// Planning tool declaration
// ---------------------------------------------------------------------------

/// Returns the `FunctionDeclaration` for the built-in `_plan` pseudo-tool.
///
/// When injected into the tool list, this gives the LLM a structured place to
/// reason/decompose tasks without polluting user-visible output. The content is
/// appended to the next turn's context but never displayed to the user.
pub fn plan_tool_declaration() -> FunctionDeclaration {
    FunctionDeclaration {
        name: "_plan".to_string(),
        description: "Write your reasoning, task decomposition, or plan to a scratchpad. \
            The content will be available in your next turn's context but will not be \
            shown to the user. Use this to think through complex tasks before acting."
            .to_string(),
        parameters: JsonSchema {
            type_value: Some("object".to_string()),
            properties: Some({
                let mut props = IndexMap::new();
                props.insert(
                    "thought".to_string(),
                    JsonSchema {
                        type_value: Some("string".to_string()),
                        description: Some(
                            "Your reasoning, plan, or task decomposition.".to_string(),
                        ),
                        ..Default::default()
                    },
                );
                props
            }),
            required: Some(vec!["thought".to_string()]),
            ..Default::default()
        },
        agent: false,
    }
}

// ---------------------------------------------------------------------------
// Core loop
// ---------------------------------------------------------------------------

/// Run the agent loop: call LLM, execute tools, iterate — up to max_turns.
///
/// This is the unified loop implementation used by both CLI (`run_directive`)
/// and REPL (`ask_inner`) when tools are configured. It replaces the old
/// `#[async_recursion]` pattern with an iterative loop that enforces a turn
/// budget, emits progress events, and supports the full async tool dispatch.
pub async fn run(input: Input, params: AgentLoopParams<'_>) -> Result<AgentLoopOutput> {
    let max_turns = params.config.read().agent_loop.max_turns;
    let mut current_input = input;
    let mut total_usage = TokenUsage::default();
    let mut last_text = String::new();

    for turn in 1..=max_turns {
        params.progress.set_turn(turn, max_turns);
        params.progress.emit(AgentLoopEvent::TurnStart { turn, max_turns });

        // On the first turn, record the initial state (matches old before_chat_completion call)
        if turn == 1 {
            params.config.write().before_chat_completion(&current_input)?;
        }

        // 1. Call the LLM (returns raw tool_calls, does not eval them)
        let (output, tool_calls) = call_llm_raw(&current_input, &params).await?;

        total_usage.add(output.usage());
        last_text = output.text.clone();

        if tool_calls.is_empty() {
            // No tools → final turn, save the message
            params
                .config
                .write()
                .after_chat_completion(&current_input, &output.text, &[])?;
            params.progress.emit(AgentLoopEvent::LoopComplete);
            return Ok(AgentLoopOutput {
                usage: total_usage,
                final_text: output.text,
            });
        }

        // Separate _plan calls from real tool calls
        let (plan_calls, real_calls): (Vec<ToolCall>, Vec<ToolCall>) =
            tool_calls.into_iter().partition(|c| c.name == "_plan");

        // Handle plan calls: emit events, log, produce "acknowledged" results
        let mut tool_results: Vec<ToolResult> = Vec::new();
        for plan_call in &plan_calls {
            let content = plan_call
                .arguments
                .get("thought")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !content.is_empty() {
                params.progress.emit(AgentLoopEvent::PlanReceived {
                    content: content.clone(),
                });
                debug!("Agent plan: {content}");
            }
            tool_results.push(ToolResult::new(
                plan_call.clone(),
                json!("acknowledged"),
            ));
        }

        // Execute real tool calls in parallel
        if !real_calls.is_empty() {
            let real_results = eval_tool_calls_parallel(
                params.config,
                real_calls,
                params.abort_signal.clone(),
                &params.progress,
            )
            .await?;
            tool_results.extend(real_results);
        }

        // If only plan calls and all produced empty results, still continue the loop
        // (the model gets "acknowledged" back and can proceed)

        params
            .config
            .write()
            .after_chat_completion(&current_input, &output.text, &tool_results)?;

        // Merge results into next input
        current_input = current_input.merge_tool_results(output.text, tool_results);

        // Budget warning
        if max_turns > 2 && turn >= max_turns - 2 {
            params.progress.emit(AgentLoopEvent::BudgetWarning { turn, max_turns });
        }
    }

    // Budget exhausted — return partial output
    params.progress.emit(AgentLoopEvent::BudgetExhausted { max_turns });
    eprintln!(
        "Warning: Agent loop reached the {}-turn limit without completing. \
         Increase with `agent_loop.max_turns` in config.yaml or AICHAT_AGENT_LOOP_MAX_TURNS=N.",
        max_turns
    );

    Ok(AgentLoopOutput {
        usage: total_usage,
        final_text: last_text,
    })
}

/// Call the LLM and return raw tool_calls (streaming or non-streaming based on input).
async fn call_llm_raw(
    input: &Input,
    params: &AgentLoopParams<'_>,
) -> Result<(ChatCompletionsOutput, Vec<ToolCall>)> {
    let client = input.create_client()?;
    let extract_code = !*IS_STDOUT_TERMINAL && params.code_mode;

    if !input.stream() || extract_code {
        call_chat_completions_raw(
            input,
            true,
            extract_code,
            client.as_ref(),
            params.abort_signal.clone(),
        )
        .await
    } else {
        call_chat_completions_streaming_raw(input, client.as_ref(), params.abort_signal.clone())
            .await
    }
}


// ---------------------------------------------------------------------------
// Observability: rendering, OSC titles, status file, notifications
// ---------------------------------------------------------------------------

use crate::config::AgentLoopConfig;
use serde::Serialize;
use std::io::Write;

/// Format an event as a trace line for stderr output.
pub fn format_trace_event(event: &AgentLoopEvent) -> Option<String> {
    match event {
        AgentLoopEvent::TurnStart { turn, max_turns } => {
            Some(format!("[turn {turn}/{max_turns}] starting"))
        }
        AgentLoopEvent::ToolStart { name, .. } => Some(format!("calling: {name}")),
        AgentLoopEvent::ToolComplete {
            name,
            duration,
            success,
        } => {
            let status = if *success { "completed" } else { "FAILED" };
            Some(format!("{name} {status} ({:.1}s)", duration.as_secs_f64()))
        }
        AgentLoopEvent::SubAgentStart { agent_name, pid } => {
            Some(format!("sub-agent {agent_name} started (PID {pid})"))
        }
        AgentLoopEvent::SubAgentComplete {
            agent_name,
            pid,
            duration,
            success,
        } => {
            let status = if *success { "completed" } else { "FAILED" };
            Some(format!(
                "sub-agent {agent_name} {status} ({:.1}s, PID {pid})",
                duration.as_secs_f64()
            ))
        }
        AgentLoopEvent::PlanReceived { content } => {
            let preview = if content.len() > 60 {
                format!("{}...", &content[..57])
            } else {
                content.clone()
            };
            Some(format!("plan: \"{preview}\""))
        }
        AgentLoopEvent::BudgetWarning { turn, max_turns } => {
            Some(format!("budget warning: turn {turn}/{max_turns}"))
        }
        AgentLoopEvent::BudgetExhausted { max_turns } => {
            Some(format!("budget exhausted at {max_turns} turns"))
        }
        AgentLoopEvent::LoopComplete => Some("done".to_string()),
    }
}

/// Format a spinner message from a progress snapshot.
pub fn format_spinner_message(snapshot: &AgentLoopSnapshot) -> String {
    if snapshot.active_tools.is_empty() {
        format!(
            "Turn {}/{} ({:.0}s)",
            snapshot.current_turn,
            snapshot.max_turns,
            snapshot.elapsed.as_secs_f64()
        )
    } else {
        let tools = snapshot.active_tools.join(", ");
        format!(
            "Turn {}/{} | {} ({:.1}s)",
            snapshot.current_turn,
            snapshot.max_turns,
            tools,
            snapshot.elapsed.as_secs_f64()
        )
    }
}

/// Build the OSC 0 terminal title string for the current state.
fn format_osc_title(event: &AgentLoopEvent, snapshot: &AgentLoopSnapshot) -> String {
    match event {
        AgentLoopEvent::LoopComplete => "aichat: done".to_string(),
        AgentLoopEvent::BudgetExhausted { .. } => "aichat: turn limit reached".to_string(),
        _ => {
            if snapshot.active_tools.is_empty() {
                format!("aichat: turn {}/{}", snapshot.current_turn, snapshot.max_turns)
            } else {
                let tools = snapshot.active_tools.join(", ");
                format!(
                    "aichat: turn {}/{} | {}",
                    snapshot.current_turn, snapshot.max_turns, tools
                )
            }
        }
    }
}

/// Emit an OSC 0/2 escape sequence to set the terminal title.
pub fn update_terminal_title(title: &str) {
    if *IS_STDOUT_TERMINAL {
        eprint!("\x1b]0;{title}\x07");
    }
}

/// Emit BEL + OSC 777 notification to the terminal.
pub fn notify_terminal(title: &str, message: &str) {
    if *IS_STDOUT_TERMINAL {
        // BEL — tmux monitor-bell picks this up
        eprint!("\x07");
        // OSC 777 — desktop notification on Ghostty, iTerm2, VS Code, rxvt-unicode
        eprint!("\x1b]777;notify;{title};{message}\x07");
    }
}

/// JSON status file contents.
#[derive(Serialize)]
struct AgentLoopStatus {
    pid: u32,
    state: String,
    turn: usize,
    max_turns: usize,
    active_tools: Vec<String>,
    elapsed_s: f64,
    updated_at: String,
}

/// Get the status file path for this process.
fn status_file_path() -> std::path::PathBuf {
    let pid = std::process::id();
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(runtime_dir).join(format!("aichat-{pid}.json"))
    } else {
        std::path::PathBuf::from(format!("/tmp/aichat-{pid}.json"))
    }
}

/// Write the status file atomically.
fn write_status_file(snapshot: &AgentLoopSnapshot, state: &str) {
    let status = AgentLoopStatus {
        pid: std::process::id(),
        state: state.to_string(),
        turn: snapshot.current_turn,
        max_turns: snapshot.max_turns,
        active_tools: snapshot.active_tools.clone(),
        elapsed_s: snapshot.elapsed.as_secs_f64(),
        updated_at: chrono_now_iso(),
    };
    let path = status_file_path();
    let tmp = path.with_extension("tmp");
    if let Ok(json) = serde_json::to_string(&status) {
        if let Ok(mut f) = std::fs::File::create(&tmp) {
            let _ = f.write_all(json.as_bytes());
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Delete the status file (called on exit).
pub fn cleanup_status_file() {
    let _ = std::fs::remove_file(status_file_path());
}

/// Simple ISO 8601 timestamp without pulling in chrono crate.
fn chrono_now_iso() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Basic UTC timestamp — good enough for status file
    format!("{secs}")
}

/// Determine the state string from an event.
fn state_from_event(event: &AgentLoopEvent) -> &'static str {
    match event {
        AgentLoopEvent::LoopComplete => "done",
        AgentLoopEvent::BudgetExhausted { .. } => "budget_exhausted",
        AgentLoopEvent::ToolStart { .. } => "working",
        AgentLoopEvent::ToolComplete { .. } => "working",
        AgentLoopEvent::TurnStart { .. } => "working",
        AgentLoopEvent::SubAgentStart { .. } => "working",
        AgentLoopEvent::SubAgentComplete { .. } => "working",
        AgentLoopEvent::PlanReceived { .. } => "working",
        AgentLoopEvent::BudgetWarning { .. } => "working",
    }
}

/// Determine if an event should trigger a notification.
fn notification_for_event(event: &AgentLoopEvent) -> Option<(&'static str, &'static str)> {
    match event {
        AgentLoopEvent::LoopComplete => Some(("aichat", "Task complete")),
        AgentLoopEvent::BudgetExhausted { .. } => Some(("aichat", "Turn limit reached")),
        _ => None,
    }
}

/// Process a single agent loop event through all observability channels.
///
/// Called by the rendering loop in the caller (run_directive / ask_inner).
pub fn render_event(
    event: &AgentLoopEvent,
    snapshot: &AgentLoopSnapshot,
    config: &AgentLoopConfig,
    spinner: &crate::utils::Spinner,
    trace_header_printed: &mut bool,
) -> Result<()> {
    // 1. Trace output (stderr)
    if config.show_trace {
        if let Some(line) = format_trace_event(event) {
            let output = if *trace_header_printed {
                format!("  [{line}]")
            } else {
                *trace_header_printed = true;
                format!("Agent loop trace:\n  [{line}]")
            };
            if *IS_STDOUT_TERMINAL {
                spinner.print_line(output)?;
            } else {
                eprintln!("{output}");
            }
        }
    }

    // 2. OSC terminal title
    if config.osc_title {
        let title = format_osc_title(event, snapshot);
        update_terminal_title(&title);
    }

    // 3. Status file
    if config.status_file {
        let state = state_from_event(event);
        write_status_file(snapshot, state);
    }

    // 4. Notifications (only on terminal events)
    if config.notify {
        if let Some((title, msg)) = notification_for_event(event) {
            notify_terminal(title, msg);
        }
    }

    Ok(())
}
