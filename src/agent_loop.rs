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

    // Route 2: Shell-exec tools (wrapped in spawn_blocking)
    let config = config.clone();
    let call = call.clone();
    tokio::task::spawn_blocking(move || call.eval_shell(&config))
        .await
        .map_err(|e| anyhow::anyhow!("Tool task panicked: {e}"))?
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

        // Intermediate turn with tool calls — after_chat_completion is a no-op
        // when tool_results is non-empty, but we call it for consistency with
        // the existing pattern (it records last_message).
        let tool_results = eval_tool_calls_parallel(
            params.config,
            tool_calls,
            params.abort_signal.clone(),
            &params.progress,
        )
        .await?;

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
