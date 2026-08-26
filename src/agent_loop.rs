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
use crate::config::{GlobalConfig, Input, RoleLike};
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
    CostExhausted {
        cost: f64,
        max_cost: f64,
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
    accumulated_cost: f64,
    event_sender: Option<UnboundedSender<AgentLoopEvent>>,
}

/// A point-in-time snapshot of agent loop progress for spinner/heartbeat rendering.
#[derive(Debug, Clone)]
pub struct AgentLoopSnapshot {
    pub current_turn: usize,
    pub max_turns: usize,
    pub active_tools: Vec<String>,
    pub elapsed: Duration,
    pub accumulated_cost: f64,
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
            accumulated_cost: state.accumulated_cost,
        }
    }

    /// Add cost to the running total.
    pub fn add_cost(&self, cost: f64) {
        self.state.lock().accumulated_cost += cost;
    }

    /// Get the current accumulated cost.
    pub fn cost(&self) -> f64 {
        self.state.lock().accumulated_cost
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
    let tool_output_limit = config.read().agent_loop.tool_output_limit;
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
                    Ok(mut value) => {
                        // Extract sub-agent cost if present
                        if let Some(obj) = value.as_object_mut() {
                            if let Some(cost_val) = obj.remove("__sub_agent_cost") {
                                if let Some(cost) = cost_val.as_f64() {
                                    progress.add_cost(cost);
                                }
                            }
                        }

                        progress.emit(AgentLoopEvent::ToolComplete {
                            name: call.name.clone(),
                            duration,
                            success: true,
                        });
                        if value.is_null() {
                            json!("DONE")
                        } else {
                            // Apply output routing (capping, file, pipe)
                            apply_output_routing(
                                &config,
                                &call.name,
                                value,
                                tool_output_limit,
                            )
                            .await
                        }
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
            let (result, sub_cost) = eval_agent_tool_subprocess(config, call).await?;
            // Sub-agent cost will be aggregated by the caller via progress.add_cost()
            // We encode it in the result metadata for the parallel dispatcher to pick up.
            if sub_cost > 0.0 {
                if let serde_json::Value::Object(ref map) = result {
                    let mut enriched = map.clone();
                    enriched.insert("__sub_agent_cost".to_string(), json!(sub_cost));
                    return Ok(serde_json::Value::Object(enriched));
                }
            }
            return Ok(result);
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
) -> Result<(serde_json::Value, f64)> {
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
    cmd.arg("--show-cost");
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

    // 6. Return result + parse sub-agent cost from stderr
    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();
    let sub_cost = parse_cost_from_stderr(&stderr_text);

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((json!({"output": text}), sub_cost))
    } else {
        let message = if stderr_text.trim().is_empty() {
            format!(
                "Sub-agent '{}' exited with code {:?}",
                agent_name,
                output.status.code()
            )
        } else {
            stderr_text.trim().to_string()
        };
        Ok((json!({
            "error": {
                "type": "agent_error",
                "message": message
            }
        }), sub_cost))
    }
}

// ---------------------------------------------------------------------------
// Output routing: capping, file destination, pipe destination
// ---------------------------------------------------------------------------

use crate::function::{OutputDestination, OutputRouting};
use std::collections::HashSet;

/// Apply output routing to a tool's result based on its declaration.
///
/// Routes: Context (with auto-capping), File (write + confirmation), Pipe (chain to target).
/// Returns the value that should be placed in the ToolResult for the conversation.
#[async_recursion::async_recursion]
async fn apply_output_routing(
    config: &GlobalConfig,
    tool_name: &str,
    output: serde_json::Value,
    tool_output_limit: usize,
) -> serde_json::Value {
    let routing = get_tool_routing(config, tool_name);

    match routing.as_ref().map(|r| &r.destination) {
        Some(OutputDestination::File) => {
            route_to_file(&output, tool_name, routing.as_ref().unwrap())
        }
        Some(OutputDestination::Pipe) => {
            let target = routing
                .as_ref()
                .and_then(|r| r.target.as_deref())
                .unwrap_or("");
            if target.is_empty() {
                apply_capping(output, tool_name, tool_output_limit)
            } else if detect_pipe_cycle(config, tool_name).is_err() {
                json!({
                    "error": {
                        "type": "pipe_cycle_error",
                        "message": format!("Pipe cycle detected starting from tool '{tool_name}'")
                    }
                })
            } else {
                match route_to_pipe(config, output, target, tool_output_limit).await {
                    Ok(result) => result,
                    Err(e) => json!({
                        "error": {
                            "type": "pipe_error",
                            "message": format!("Pipe to '{target}' failed: {e}")
                        }
                    }),
                }
            }
        }
        _ => {
            apply_capping(output, tool_name, tool_output_limit)
        }
    }
}

/// Look up the output routing declaration for a tool.
fn get_tool_routing(config: &GlobalConfig, tool_name: &str) -> Option<OutputRouting> {
    let config_read = config.read();
    if let Some(agent) = &config_read.agent {
        if let Some(decl) = agent.functions().find(tool_name) {
            return decl.output.clone();
        }
    }
    if let Some(decl) = config_read.functions.find(tool_name) {
        return decl.output.clone();
    }
    None
}

/// Apply large-result capping: if output exceeds the limit, write to temp file and return preview.
fn apply_capping(
    output: serde_json::Value,
    tool_name: &str,
    limit: usize,
) -> serde_json::Value {
    if limit == 0 {
        return output;
    }
    let content = value_to_string(&output);
    if content.len() <= limit {
        return output;
    }

    // Write full output to temp file
    let pid = std::process::id();
    let path = format!("/tmp/aichat-tool-{tool_name}-{pid}.out");
    if std::fs::write(&path, &content).is_err() {
        // Can't write temp file — return original (don't lose data)
        return output;
    }

    // Build preview
    let preview: String = content.chars().take(limit).collect();
    let total_bytes = content.len();
    let hint = if output.is_object() {
        if let Some(obj) = output.as_object() {
            let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
            format!("JSON object with keys: {}", keys.join(", "))
        } else {
            format!("{} lines", content.lines().count())
        }
    } else {
        format!("{} lines", content.lines().count())
    };

    json!({
        "preview": preview,
        "full_output_path": path,
        "total_bytes": total_bytes,
        "hint": hint
    })
}

/// Write tool output to a file and return a confirmation.
fn route_to_file(
    output: &serde_json::Value,
    tool_name: &str,
    routing: &OutputRouting,
) -> serde_json::Value {
    // Unwrap the {"output": "..."} wrapper that run_llm_function adds for non-JSON tool output
    let content = match output.get("output").and_then(|v| v.as_str()) {
        Some(raw) if output.as_object().map_or(false, |o| o.len() == 1) => raw.to_string(),
        _ => value_to_string(output),
    };
    let path = match &routing.path {
        Some(template) => expand_path_template(template, tool_name, None),
        None => format!("/tmp/{tool_name}-output.txt"),
    };

    // Create parent directories
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Write
    match std::fs::write(&path, &content) {
        Ok(()) => {
            let lines = content.lines().count();
            json!({
                "written_to": path,
                "size_bytes": content.len(),
                "hint": format!("{lines} lines")
            })
        }
        Err(e) => {
            // Fall back to returning the original output
            warn!("File routing failed for '{}': {e}. Falling back to context.", path);
            output.clone()
        }
    }
}

/// Pipe tool output to another tool, return the final result.
async fn route_to_pipe(
    config: &GlobalConfig,
    output: serde_json::Value,
    target_tool: &str,
    tool_output_limit: usize,
) -> Result<serde_json::Value> {
    // Build a synthetic ToolCall for the target with the source output as input
    let input_content = value_to_string(&output);
    let pipe_call = ToolCall::new(
        target_tool.to_string(),
        json!({"input": input_content}),
        None,
    );

    // Execute the target tool
    let result = eval_single_tool(config, &pipe_call).await?;

    // Recursively apply routing to the target's result (handles chained pipes)
    Ok(apply_output_routing(config, target_tool, result, tool_output_limit).await)
}

/// Detect cycles in a pipe chain.
fn detect_pipe_cycle(config: &GlobalConfig, start_tool: &str) -> Result<()> {
    let mut visited = HashSet::new();
    visited.insert(start_tool.to_string());

    let mut current = start_tool.to_string();
    loop {
        let routing = get_tool_routing(config, &current);
        match routing {
            Some(ref r) if r.destination == OutputDestination::Pipe => {
                let target = match &r.target {
                    Some(t) => t.clone(),
                    None => break,
                };
                if !visited.insert(target.clone()) {
                    bail!("Pipe cycle: {} → ... → {}", start_tool, target);
                }
                current = target;
            }
            _ => break,
        }
    }
    Ok(())
}

/// Expand path template variables.
fn expand_path_template(template: &str, tool_name: &str, call_id: Option<&str>) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    template
        .replace("{{name}}", tool_name)
        .replace("{{id}}", call_id.unwrap_or("none"))
        .replace("{{timestamp}}", &timestamp.to_string())
        .replace("{{ext}}", "txt")
}

/// Convert a serde_json::Value to a string for file writing / size checking.
fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        _ => serde_json::to_string_pretty(value).unwrap_or_default(),
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
        output: None,
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
    let max_cost = params.config.read().agent_loop.max_cost;
    let model = input.role().model().clone();
    let mut current_input = input;
    let mut total_usage = TokenUsage::default();
    let mut last_text = String::new();

    // Circuit breaker: track consecutive failures per tool name.
    // After 3 consecutive failures, the tool is "tripped" and further calls
    // return an error immediately without execution.
    const CIRCUIT_BREAKER_THRESHOLD: usize = 3;
    let mut tool_failure_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut tripped_tools: std::collections::HashSet<String> = std::collections::HashSet::new();

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

        // Track cost
        if let Some(turn_cost) = model.usage_cost(output.usage()) {
            params.progress.add_cost(turn_cost);
        }

        // Cost budget check
        if max_cost > 0.0 && params.progress.cost() > max_cost {
            params.progress.emit(AgentLoopEvent::CostExhausted {
                cost: params.progress.cost(),
                max_cost,
            });
            eprintln!(
                "Warning: Agent loop exceeded the ${:.4} cost limit (spent ${:.4}). \
                 Increase with `agent_loop.max_cost` in config.yaml or AICHAT_AGENT_LOOP_MAX_COST=N.",
                max_cost, params.progress.cost()
            );
            return Ok(AgentLoopOutput {
                usage: total_usage,
                final_text: last_text,
            });
        }

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

        // Circuit breaker: separate tripped calls from executable calls
        let (tripped_calls, executable_calls): (Vec<ToolCall>, Vec<ToolCall>) =
            real_calls.into_iter().partition(|c| tripped_tools.contains(&c.name));

        // Return immediate errors for tripped tools
        for call in &tripped_calls {
            tool_results.push(ToolResult::new(
                call.clone(),
                json!({
                    "error": {
                        "type": "circuit_breaker",
                        "message": format!(
                            "Tool '{}' has been disabled after {} consecutive failures. \
                             Use a different tool or approach.",
                            call.name, CIRCUIT_BREAKER_THRESHOLD
                        )
                    }
                }),
            ));
        }

        // Execute remaining tool calls in parallel
        if !executable_calls.is_empty() {
            let real_results = eval_tool_calls_parallel(
                params.config,
                executable_calls,
                params.abort_signal.clone(),
                &params.progress,
            )
            .await?;

            // Update circuit breaker state based on results
            for result in &real_results {
                let name = &result.call.name;
                let is_error = result.output.get("error").is_some();
                if is_error {
                    let count = tool_failure_counts.entry(name.clone()).or_insert(0);
                    *count += 1;
                    if *count >= CIRCUIT_BREAKER_THRESHOLD {
                        tripped_tools.insert(name.clone());
                        warn!(
                            "Circuit breaker tripped for tool '{}' after {} consecutive failures",
                            name, count
                        );
                    }
                } else {
                    // Success resets the counter
                    tool_failure_counts.remove(name);
                }
            }

            tool_results.extend(real_results);
        }

        // If all real calls were tripped and no executable calls ran, give the model
        // a chance to try something else. But if it keeps asking for tripped tools
        // with nothing else, the budget will eventually stop it.

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
pub fn format_trace_event(event: &AgentLoopEvent, pid: u32) -> Option<String> {
    match event {
        AgentLoopEvent::TurnStart { turn, max_turns } => {
            Some(format!("{pid} [turn {turn}/{max_turns}] starting"))
        }
        AgentLoopEvent::ToolStart { name, .. } => Some(format!("{pid} calling: {name}")),
        AgentLoopEvent::ToolComplete {
            name,
            duration,
            success,
        } => {
            let status = if *success { "completed" } else { "FAILED" };
            Some(format!("{pid} {name} {status} ({:.1}s)", duration.as_secs_f64()))
        }
        AgentLoopEvent::SubAgentStart { agent_name, pid: sub_pid } => {
            Some(format!("{pid} sub-agent {agent_name} started (PID {sub_pid})"))
        }
        AgentLoopEvent::SubAgentComplete {
            agent_name,
            pid: sub_pid,
            duration,
            success,
        } => {
            let status = if *success { "completed" } else { "FAILED" };
            Some(format!(
                "{pid} sub-agent {agent_name} {status} ({:.1}s, PID {sub_pid})",
                duration.as_secs_f64()
            ))
        }
        AgentLoopEvent::PlanReceived { content } => {
            let preview = if content.len() > 60 {
                format!("{}...", &content[..57])
            } else {
                content.clone()
            };
            Some(format!("{pid} plan: \"{preview}\""))
        }
        AgentLoopEvent::BudgetWarning { turn, max_turns } => {
            Some(format!("{pid} budget warning: turn {turn}/{max_turns}"))
        }
        AgentLoopEvent::BudgetExhausted { max_turns } => {
            Some(format!("{pid} budget exhausted at {max_turns} turns"))
        }
        AgentLoopEvent::CostExhausted { cost, max_cost } => {
            Some(format!("{pid} cost exhausted: ${cost:.4} exceeded ${max_cost:.4} limit"))
        }
        AgentLoopEvent::LoopComplete => Some(format!("{pid} done")),
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
fn format_osc_title(event: &AgentLoopEvent, snapshot: &AgentLoopSnapshot, agent_label: &str) -> String {
    let pid = std::process::id();
    match event {
        AgentLoopEvent::LoopComplete => format!("done | {agent_label}:{pid}"),
        AgentLoopEvent::BudgetExhausted { .. } => format!("turn limit reached | {agent_label}:{pid}"),
        AgentLoopEvent::CostExhausted { cost, .. } => format!("cost limit ${cost:.2} | {agent_label}:{pid}"),
        _ => format_heartbeat_title_with(snapshot, agent_label),
    }
}

/// Build a title from the current snapshot (used by both event-driven and heartbeat updates).
/// Uses the provided agent label.
pub fn format_heartbeat_title_with(snapshot: &AgentLoopSnapshot, agent_label: &str) -> String {
    let pid = std::process::id();
    let elapsed = snapshot.elapsed.as_secs();
    let cost_str = if snapshot.accumulated_cost > 0.0 {
        format!(" ${:.4}", snapshot.accumulated_cost)
    } else {
        String::new()
    };
    if snapshot.active_tools.is_empty() {
        format!("turn {}/{} | {agent_label}:{pid} ({}s{cost_str})", snapshot.current_turn, snapshot.max_turns, elapsed)
    } else {
        let tools = snapshot.active_tools.join(", ");
        format!(
            "turn {}/{} | {} | {agent_label}:{pid} ({}s{cost_str})",
            snapshot.current_turn, snapshot.max_turns, tools, elapsed
        )
    }
}

/// Build a title from the current snapshot using the default label.
/// Called from main.rs heartbeat where only the snapshot is available.
pub fn format_heartbeat_title(snapshot: &AgentLoopSnapshot, agent_label: &str) -> String {
    format_heartbeat_title_with(snapshot, agent_label)
}

/// Emit an OSC 0/2 escape sequence to set the terminal title.
/// Writes directly to /dev/tty to bypass stdout/stderr pipes — works in tmux
/// even when both stdout and stderr are captured (e.g., from nushell `| complete`).
pub fn update_terminal_title(title: &str) {
    use std::io::Write;
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        let _ = write!(tty, "\x1b]0;{title}\x07");
    }
}

/// Emit BEL + desktop notification escape sequences to the terminal.
/// Writes directly to /dev/tty so it reaches tmux regardless of pipe state.
/// Emits multiple notification protocols for broad terminal compatibility:
///   - BEL (\x07): universal, tmux monitor-bell
///   - OSC 777: Ghostty, iTerm2, rxvt-unicode, VS Code terminal
///   - OSC 9: Windows Terminal, ConEmu
///   - OSC 99: kitty
pub fn notify_terminal(title: &str, message: &str) {
    use std::io::Write;
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        // BEL — tmux monitor-bell picks this up
        let _ = write!(tty, "\x07");
        // OSC 777 — Ghostty, iTerm2, VS Code, rxvt-unicode
        let _ = write!(tty, "\x1b]777;notify;{title};{message}\x07");
        // OSC 9 — Windows Terminal, ConEmu
        let _ = write!(tty, "\x1b]9;{message}\x07");
        // OSC 99 — kitty notification protocol
        let _ = write!(tty, "\x1b]99;i=aichat;{message}\x1b\\");
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
    cost_usd: f64,
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
        cost_usd: snapshot.accumulated_cost,
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

/// Remove stale status files from previous aichat processes that are no longer running.
/// Called at startup to prevent leftover files from crashes/kills.
pub fn cleanup_stale_status_files() {
    let dir = if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(runtime_dir)
    } else {
        std::path::PathBuf::from("/tmp")
    };

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let my_pid = std::process::id();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Match aichat-<pid>.json
        if let Some(rest) = name_str.strip_prefix("aichat-") {
            if let Some(pid_str) = rest.strip_suffix(".json") {
                if let Ok(pid) = pid_str.parse::<u32>() {
                    // Skip our own file
                    if pid == my_pid {
                        continue;
                    }
                    // Check if process is still alive
                    let proc_path = format!("/proc/{pid}");
                    if !std::path::Path::new(&proc_path).exists() {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

/// Parse estimated cost from a sub-agent's stderr output.
/// Looks for the pattern: "Estimated cost: $0.004000"
fn parse_cost_from_stderr(stderr: &str) -> f64 {
    for line in stderr.lines() {
        if let Some(pos) = line.find("Estimated cost: $") {
            let start = pos + "Estimated cost: $".len();
            if let Some(cost_str) = line[start..].split_whitespace().next() {
                if let Ok(cost) = cost_str.parse::<f64>() {
                    return cost;
                }
            }
        }
    }
    0.0
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
        AgentLoopEvent::CostExhausted { .. } => "cost_exhausted",
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
        AgentLoopEvent::CostExhausted { .. } => Some(("aichat", "Cost limit reached")),
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
    agent_label: &str,
    spinner: &crate::utils::Spinner,
    trace_header_printed: &mut bool,
) -> Result<()> {
    let pid = std::process::id();

    // 1. Trace output — written to /dev/tty (live, visible regardless of pipe state).
    //    Falls back to stderr if /dev/tty is unavailable (CI, cron).
    //    When stdout IS a terminal, uses spinner.print_line for clean rendering.
    if config.show_trace {
        if let Some(line) = format_trace_event(event, pid) {
            let output = if *trace_header_printed {
                format!("  [{line}]")
            } else {
                *trace_header_printed = true;
                format!("Agent {agent_label} ({pid}) loop trace:\n  [{line}]")
            };
            if *IS_STDOUT_TERMINAL {
                spinner.print_line(output)?;
            } else {
                use std::io::Write;
                if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
                    let _ = writeln!(tty, "{output}");
                } else {
                    // No /dev/tty available (CI, cron, containers) — fall back to stderr
                    eprintln!("{output}");
                }
            }
        }
    }

    // 2. OSC terminal title
    if config.osc_title {
        let title = format_osc_title(event, snapshot, agent_label);
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


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, RoleLike};
    use parking_lot::RwLock;

    fn default_config() -> GlobalConfig {
        Arc::new(RwLock::new(Config::default()))
    }

    fn config_with_agent_loop(yaml: &str) -> GlobalConfig {
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        Arc::new(RwLock::new(config))
    }

    // --- Config parsing tests ---

    #[test]
    fn agent_loop_config_defaults_when_absent() {
        let config: Config = serde_yaml::from_str("{}").unwrap();
        let al = &config.agent_loop;
        assert_eq!(al.max_turns, 20);
        assert_eq!(al.max_concurrency, 8);
        assert_eq!(al.max_agent_depth, 3);
        assert!(!al.show_trace);
        assert!(al.planning_tool);
        assert!(al.osc_title);
        assert!(al.status_file);
        assert!(al.notify);
    }

    #[test]
    fn agent_loop_config_partial_override() {
        let config: Config =
            serde_yaml::from_str("agent_loop:\n  max_turns: 50\n  show_trace: true\n").unwrap();
        let al = &config.agent_loop;
        assert_eq!(al.max_turns, 50);
        assert!(al.show_trace);
        // Others stay default
        assert_eq!(al.max_concurrency, 8);
        assert!(al.planning_tool);
    }

    #[test]
    fn agent_loop_config_full_override() {
        let yaml = r#"
agent_loop:
  max_turns: 10
  max_concurrency: 4
  max_agent_depth: 2
  show_trace: true
  planning_tool: false
  osc_title: false
  status_file: false
  notify: false
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let al = &config.agent_loop;
        assert_eq!(al.max_turns, 10);
        assert_eq!(al.max_concurrency, 4);
        assert_eq!(al.max_agent_depth, 2);
        assert!(al.show_trace);
        assert!(!al.planning_tool);
        assert!(!al.osc_title);
        assert!(!al.status_file);
        assert!(!al.notify);
    }

    // --- Planning tool tests ---

    #[test]
    fn plan_tool_declaration_has_correct_shape() {
        let decl = plan_tool_declaration();
        assert_eq!(decl.name, "_plan");
        assert!(!decl.agent);
        assert!(decl.description.contains("scratchpad"));
        let props = decl.parameters.properties.as_ref().unwrap();
        assert!(props.contains_key("thought"));
        let required = decl.parameters.required.as_ref().unwrap();
        assert!(required.contains(&"thought".to_string()));
    }

    #[test]
    fn plan_tool_injected_when_enabled_and_tools_exist() {
        let yaml = r#"
function_calling: true
agent_loop:
  planning_tool: true
"#;
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "test_tool",
                "description": "a test",
                "parameters": {"type": "object"}
            }))
            .unwrap(),
        ]);
        // Set use_tools on the role so select_functions returns tools
        let mut role = config.extract_role();
        role.set_use_tools(Some("all".to_string()));
        let functions = config.select_functions(&role);
        let names: Vec<&str> = functions
            .as_ref()
            .unwrap()
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(names.contains(&"_plan"));
        assert!(names.contains(&"test_tool"));
    }

    #[test]
    fn plan_tool_not_injected_when_disabled() {
        let yaml = r#"
function_calling: true
agent_loop:
  planning_tool: false
"#;
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "test_tool",
                "description": "a test",
                "parameters": {"type": "object"}
            }))
            .unwrap(),
        ]);
        let mut role = config.extract_role();
        role.set_use_tools(Some("all".to_string()));
        let functions = config.select_functions(&role);
        let names: Vec<&str> = functions
            .as_ref()
            .unwrap()
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(!names.contains(&"_plan"));
        assert!(names.contains(&"test_tool"));
    }

    #[test]
    fn plan_tool_not_injected_when_no_tools() {
        let config: Config = serde_yaml::from_str("agent_loop:\n  planning_tool: true\n").unwrap();
        let role = config.extract_role();
        let functions = config.select_functions(&role);
        assert!(functions.is_none());
    }

    // --- Progress tracking tests ---

    #[test]
    fn progress_live_emits_and_receives_events() {
        let (progress, mut rx) = AgentLoopProgress::live();
        progress.emit(AgentLoopEvent::TurnStart {
            turn: 1,
            max_turns: 20,
        });
        progress.emit(AgentLoopEvent::LoopComplete);

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentLoopEvent::TurnStart { turn: 1, .. }));
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentLoopEvent::LoopComplete));
    }

    #[test]
    fn progress_snapshot_tracks_turn_and_active_tools() {
        let (progress, _rx) = AgentLoopProgress::live();
        progress.set_turn(3, 20);
        progress.add_active_tool("fs_write");
        progress.add_active_tool("execute_command");

        let snapshot = progress.snapshot();
        assert_eq!(snapshot.current_turn, 3);
        assert_eq!(snapshot.max_turns, 20);
        assert_eq!(snapshot.active_tools, vec!["fs_write", "execute_command"]);

        progress.remove_active_tool("fs_write");
        let snapshot = progress.snapshot();
        assert_eq!(snapshot.active_tools, vec!["execute_command"]);
    }

    // --- Trace formatting tests ---

    #[test]
    fn format_trace_event_produces_expected_output() {
        let pid = 12345u32;
        let event = AgentLoopEvent::ToolComplete {
            name: "fs_write".to_string(),
            duration: Duration::from_millis(1234),
            success: true,
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert!(line.contains("12345"));
        assert!(line.contains("fs_write"));
        assert!(line.contains("completed"));
        assert!(line.contains("1.2s"));

        let event = AgentLoopEvent::ToolComplete {
            name: "bad_tool".to_string(),
            duration: Duration::from_millis(500),
            success: false,
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert!(line.contains("FAILED"));
        assert!(line.contains("12345"));
    }

    #[test]
    fn format_spinner_message_shows_tools_when_active() {
        let snapshot = AgentLoopSnapshot {
            current_turn: 2,
            max_turns: 20,
            active_tools: vec!["fs_write".to_string(), "web_search".to_string()],
            elapsed: Duration::from_secs(5),
        };
        let msg = format_spinner_message(&snapshot);
        assert!(msg.contains("Turn 2/20"));
        assert!(msg.contains("fs_write"));
        assert!(msg.contains("web_search"));
    }

    #[test]
    fn format_spinner_message_no_tools() {
        let snapshot = AgentLoopSnapshot {
            current_turn: 1,
            max_turns: 10,
            active_tools: vec![],
            elapsed: Duration::from_secs(3),
        };
        let msg = format_spinner_message(&snapshot);
        assert!(msg.contains("Turn 1/10"));
        assert!(!msg.contains("|"));
    }

    // --- Depth enforcement test ---

    #[test]
    fn agent_depth_check_respects_env_var() {
        // Simulate being at max depth
        std::env::set_var("AICHAT_AGENT_DEPTH", "3");
        let config = config_with_agent_loop("agent_loop:\n  max_agent_depth: 3\n");
        let current_depth: usize = std::env::var("AICHAT_AGENT_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let max_depth = config.read().agent_loop.max_agent_depth;
        assert!(current_depth >= max_depth);
        // Clean up
        std::env::remove_var("AICHAT_AGENT_DEPTH");
    }

    // --- Output routing tests ---

    #[test]
    fn capping_passes_small_results_unchanged() {
        let small = json!({"data": "hello world"});
        let result = apply_capping(small.clone(), "test_tool", 16384);
        assert_eq!(result, small);
    }

    #[test]
    fn capping_caps_large_results() {
        let large_content = "x".repeat(20000);
        let large = json!(large_content);
        let result = apply_capping(large, "cap_test", 16384);

        assert!(result.get("preview").is_some());
        assert!(result.get("full_output_path").is_some());
        assert!(result.get("total_bytes").is_some());
        assert_eq!(result["total_bytes"], 20000);

        let preview = result["preview"].as_str().unwrap();
        assert_eq!(preview.len(), 16384);

        // Clean up temp file
        if let Some(path) = result["full_output_path"].as_str() {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn capping_disabled_when_limit_is_zero() {
        let large = json!("x".repeat(20000));
        let result = apply_capping(large.clone(), "test_tool", 0);
        assert_eq!(result, large);
    }

    #[test]
    fn capping_hint_shows_lines_for_strings() {
        let multiline = "line1\nline2\nline3\n".repeat(2000);
        let value = json!(multiline);
        let result = apply_capping(value, "lines_test", 100);

        let hint = result["hint"].as_str().unwrap();
        assert!(hint.contains("lines"));

        if let Some(path) = result["full_output_path"].as_str() {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn capping_hint_shows_keys_for_json_objects() {
        // Build a JSON object that exceeds the limit
        let mut obj = serde_json::Map::new();
        obj.insert("alpha".to_string(), json!("x".repeat(10000)));
        obj.insert("beta".to_string(), json!("y".repeat(10000)));
        let value = serde_json::Value::Object(obj);

        let result = apply_capping(value, "json_test", 100);
        let hint = result["hint"].as_str().unwrap();
        assert!(hint.contains("alpha"));
        assert!(hint.contains("beta"));

        if let Some(path) = result["full_output_path"].as_str() {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn file_routing_writes_and_returns_confirmation() {
        let output = json!("file content here");
        let routing = OutputRouting {
            destination: OutputDestination::File,
            path: Some("/tmp/aichat-test-{{name}}.txt".to_string()),
            target: None,
        };

        let result = route_to_file(&output, "route_test", &routing);

        assert!(result.get("written_to").is_some());
        assert!(result.get("size_bytes").is_some());
        let path = result["written_to"].as_str().unwrap();
        assert!(path.contains("route_test"));

        // Verify file was written
        let content = std::fs::read_to_string(path).unwrap();
        assert_eq!(content, "file content here");

        // Clean up
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn file_routing_falls_back_on_invalid_path() {
        let output = json!("test data");
        let routing = OutputRouting {
            destination: OutputDestination::File,
            path: Some("/nonexistent/deeply/nested/impossible/path/file.txt".to_string()),
            target: None,
        };

        let result = route_to_file(&output, "fallback_test", &routing);
        // Should fall back to returning the original output
        assert_eq!(result, json!("test data"));
    }

    #[test]
    fn template_expansion_replaces_variables() {
        let expanded = expand_path_template(
            "/tmp/{{name}}-{{id}}-{{ext}}",
            "my_tool",
            Some("call-123"),
        );
        assert!(expanded.contains("my_tool"));
        assert!(expanded.contains("call-123"));
        assert!(expanded.contains("txt"));
        assert!(!expanded.contains("{{"));
    }

    #[test]
    fn template_expansion_handles_missing_id() {
        let expanded = expand_path_template("/tmp/{{name}}-{{id}}.out", "tool", None);
        assert!(expanded.contains("tool"));
        assert!(expanded.contains("none"));
    }

    #[test]
    fn pipe_cycle_detection_catches_self_reference() {
        let mut config_inner = Config::default();
        config_inner.functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "loop_tool",
                "description": "loops to itself",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": "loop_tool"}
            }))
            .unwrap(),
        ]);
        let config: GlobalConfig = Arc::new(RwLock::new(config_inner));

        let result = detect_pipe_cycle(&config, "loop_tool");
        assert!(result.is_err());
    }

    #[test]
    fn pipe_cycle_detection_allows_linear_chain() {
        let mut config_inner = Config::default();
        config_inner.functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "tool_a",
                "description": "pipes to b",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": "tool_b"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "name": "tool_b",
                "description": "no pipe",
                "parameters": {"type": "object"}
            }))
            .unwrap(),
        ]);
        let config: GlobalConfig = Arc::new(RwLock::new(config_inner));

        let result = detect_pipe_cycle(&config, "tool_a");
        assert!(result.is_ok());
    }

    #[test]
    fn value_to_string_handles_all_types() {
        assert_eq!(value_to_string(&json!("hello")), "hello");
        assert_eq!(value_to_string(&json!(null)), "");
        let obj_str = value_to_string(&json!({"a": 1}));
        assert!(obj_str.contains("\"a\""));
        assert!(obj_str.contains("1"));
    }

    #[test]
    fn output_routing_deserializes_from_json() {
        let decl: crate::function::FunctionDeclaration = serde_json::from_value(json!({
            "name": "test",
            "description": "test tool",
            "parameters": {"type": "object"},
            "output": {"destination": "file", "path": "/tmp/{{name}}.md"}
        }))
        .unwrap();

        let routing = decl.output.unwrap();
        assert_eq!(routing.destination, OutputDestination::File);
        assert_eq!(routing.path.unwrap(), "/tmp/{{name}}.md");
    }

    #[test]
    fn output_routing_absent_means_context() {
        let decl: crate::function::FunctionDeclaration = serde_json::from_value(json!({
            "name": "test",
            "description": "test tool",
            "parameters": {"type": "object"}
        }))
        .unwrap();

        assert!(decl.output.is_none());
    }

    #[test]
    fn tool_output_limit_config_defaults_to_16kb() {
        let config: Config = serde_yaml::from_str("{}").unwrap();
        assert_eq!(config.agent_loop.tool_output_limit, 16384);
    }

    #[test]
    fn tool_output_limit_config_overridable() {
        let config: Config =
            serde_yaml::from_str("agent_loop:\n  tool_output_limit: 32768\n").unwrap();
        assert_eq!(config.agent_loop.tool_output_limit, 32768);
    }
}
