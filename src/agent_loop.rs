//! Client-side agent loop: iterative tool-call execution for any LLM provider.
//!
//! This module provides a unified, provider-agnostic agent loop that replaces the
//! recursive `run_directive` / `ask_inner` pattern. It supports parallel tool execution,
//! configurable turn budgets, sub-agent subprocess delegation, live progress reporting,
//! external observability signals (OSC titles, status files, notifications), and a
//! built-in planning tool.

use crate::client::TokenUsage;
use crate::config::{GlobalConfig, Input};
use crate::function::{FunctionDeclaration, JsonSchema, ToolCall, ToolResult};
use crate::utils::AbortSignal;

use anyhow::Result;
use indexmap::IndexMap;
use parking_lot::Mutex;
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
// Core loop (placeholder — will be fully implemented in Phase B, Task 5)
// ---------------------------------------------------------------------------

/// Run the agent loop: call LLM, execute tools, iterate — up to max_turns.
///
/// This is a transitional placeholder that performs a single LLM call without
/// looping. The full iterative implementation comes in Phase B (Task 5).
pub async fn run(_input: Input, _params: AgentLoopParams<'_>) -> Result<AgentLoopOutput> {
    // TODO(phase-b): Replace with iterative loop implementation.
    // For now, return empty output — callers still use the old recursive path.
    Ok(AgentLoopOutput {
        usage: TokenUsage::default(),
        final_text: String::new(),
    })
}
