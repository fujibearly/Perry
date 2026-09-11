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
use crate::config::{Config, GlobalConfig, Input, RoleLike};
use crate::escalation::EscalationTransport;
use crate::function::{FunctionDeclaration, JsonSchema, ToolCall, ToolResult};
use crate::utils::*;

use anyhow::{bail, Result};
use futures_util::future::join_all;
use indexmap::IndexMap;
use parking_lot::Mutex;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
#[allow(dead_code)]
pub struct AgentLoopOutput {
    pub usage: TokenUsage,
    pub final_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogDirection {
    Request,
    Response,
}

/// Events emitted during the agent loop for observability.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AgentLoopEvent {
    TurnStart {
        turn: usize,
        max_turns: usize,
    },
    DialogBlock {
        agent: String,
        pid: u32,
        turn: usize,
        max_turns: usize,
        direction: DialogDirection,
        content: String,
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
    /// A tool call was refused by a safety gate (#6a capability mask, or #6b
    /// authority ceiling / protected policy) before the tool ran. Distinct from
    /// `ToolComplete` so the trace does not misleadingly say "completed" for an
    /// action that never executed.
    ToolBlocked {
        name: String,
        reason: String,
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
    // --- Safety Events ---
    PolicyRuleMatched {
        name: String,
        outcome: String,
    },
    SafetyGatePassed {
        name: String,
        comparison: String,
    },
    RiskAssessmentStart {
        name: String,
        model: String,
    },
    RiskAssessmentComplete {
        name: String,
        tier: String,
        confidence: String,
        rationale: String,
    },
    RiskAssessmentError {
        name: String,
        error: String,
    },
    RiskAssessmentCacheHit {
        name: String,
        cached_floor: String,
        rationale: Option<String>,
    },
    EscalationDispatched {
        name: String,
        target: String,
        reason: String,
    },
    EscalationVerdictReceived {
        name: String,
        decision: String,
    },
    HumanPromptRequested {
        name: String,
        blast_radius: String,
        reason: String,
    },
    HumanVerdictReceived {
        name: String,
        decision: String,
    },
    RollbackJournalRecorded {
        name: String,
        entry_id: String,
        entry: Option<Box<crate::safety::RollbackJournalEntry>>,
    },
    PreflightReversibilityApplied {
        name: String,
        mechanism: String,
        stepped_down_to: String,
    },
    CapabilityBlocked {
        name: String,
        unwound: bool,
    },
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
            let _ = sender.send(event.clone());
        }
        if let Some(Some(client)) = CHILD_CLIENT.get() {
            if client.is_connected() {
                let event_val = serde_json::json!({ "event": format!("{event:?}") });
                client.send_event(event_val);
            }
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
    risk_cache: Option<std::sync::Arc<parking_lot::Mutex<crate::safety::RiskCache>>>,
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
            let risk_cache = risk_cache.clone();
            async move {
                let _permit = semaphore.acquire().await.unwrap();
                let start = Instant::now();
                progress.emit(AgentLoopEvent::ToolStart {
                    name: call.name.clone(),
                    id: call.id.clone(),
                });
                progress.add_active_tool(&call.name);

                let result = eval_single_tool(&config, &call, risk_cache.as_ref(), Some(&progress)).await;
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

                        // A safety gate (#6a/#6b) returns its denial as an Ok
                        // result *without running the tool*. Emit a distinct
                        // `ToolBlocked` event (not `ToolComplete`) so the trace
                        // is truthful, and skip output routing (nothing ran).
                        if let Some(reason) = safety_block_reason(&value) {
                            progress.emit(AgentLoopEvent::ToolBlocked {
                                name: call.name.clone(),
                                reason,
                            });
                            progress.remove_active_tool(&call.name);
                            return ToolResult::new(call, value);
                        }

                        let is_error = value.is_object() && value.get("error").is_some();
                        progress.emit(AgentLoopEvent::ToolComplete {
                            name: call.name.clone(),
                            duration,
                            success: !is_error,
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
                    Err(e) => {
                        progress.emit(AgentLoopEvent::ToolComplete {
                            name: call.name.clone(),
                            duration,
                            success: false,
                        });
                        json!({
                            "error": {
                                "type": "tool_execution_error",
                                "message": format!("The tool call failed: {e}. Fix its arguments or choose another tool.")
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
    Ok(results)
}

/// Whether this process runs under a read-only capability mask (backlog #6a).
///
/// The mask is set on every spawned sub-agent via `AICHAT_CAPABILITY_MASK=readonly`.
/// The top-level process has no such env var and is therefore unmasked.
fn under_readonly_mask() -> bool {
    std::env::var("AICHAT_CAPABILITY_MASK")
        .map(|v| v.eq_ignore_ascii_case("readonly"))
        .unwrap_or(false)
}

/// Resolve a tool's declared safety classification from config.
///
/// Looks in the active agent's functions first, then the global function set.
/// MCP-sourced tools (and any tool with no `mode`) resolve to `Unclassified`.
fn tool_safety_class(config: &GlobalConfig, tool_name: &str) -> crate::function::SafetyClass {
    use crate::function::SafetyClass;
    let config_read = config.read();
    if let Some(agent) = &config_read.agent {
        if let Some(decl) = agent.functions().find(tool_name) {
            return decl.safety_class();
        }
    }
    if let Some(decl) = config_read.functions.find(tool_name) {
        return decl.safety_class();
    }
    // Unknown here (e.g. MCP tools live in a separate registry) → unclassified,
    // the most conservative disposition.
    SafetyClass::Unclassified
}

/// If the capability mask forbids this tool, return the structured denial result.
///
/// Returns `None` when the call is permitted (unmasked process, or a `readonly`
/// tool). Returns `Some(error_json)` when a masked (sub-agent) process attempts a
/// `mutating` or `unclassified` tool. The `_plan` pseudo-tool is always permitted.
fn capability_denied_result(
    config: &GlobalConfig,
    tool_name: &str,
) -> Option<serde_json::Value> {
    use crate::function::SafetyClass;

    // The planning scratchpad is internal and never mutates state.
    if tool_name == "_plan" {
        return None;
    }
    // Decision (B): delegating to a sub-agent is orchestration, not actuation.
    // A masked agent may still delegate — the grandchild inherits the mask and
    // its own actions stay gated. So delegation is not blocked by the mask.
    if call_targets_agent(config, tool_name) {
        return None;
    }
    if !under_readonly_mask() {
        return None;
    }
    let class = tool_safety_class(config, tool_name);
    if class.allowed_under_readonly_mask() {
        return None;
    }

    let (reason, message) = match class {
        SafetyClass::Unclassified => (
            "unclassified",
            format!(
                "Tool '{tool_name}' is unclassified (no safety mode declared) and is \
                 reserved to the top-level operator. A sub-agent running under a \
                 read-only capability mask cannot execute it. Perform read-only \
                 triage and return findings to your caller for actuation."
            ),
        ),
        _ => (
            "mutating",
            format!(
                "Tool '{tool_name}' is a mutating tool. A sub-agent running under a \
                 read-only capability mask cannot execute it. Perform read-only \
                 triage and return findings to your caller for actuation."
            ),
        ),
    };

    Some(json!({
        "error": {
            "type": "capability_denied",
            "reason": reason,
            "message": message
        }
    }))
}

/// This process's current autonomous authority ceiling (backlog #6b).
///
/// A spawned sub-agent reads `AICHAT_AUTHORITY_CEILING` (set by its parent). The
/// top-level process (no such env var) uses the configured `safety.default_ceiling`.
/// An unparseable env value fails safe to the minimal ceiling (`Safe`).
fn current_authority_ceiling(config: &GlobalConfig) -> crate::safety::AuthorityCeiling {
    use crate::safety::AuthorityCeiling;
    if let Ok(v) = std::env::var("AICHAT_AUTHORITY_CEILING") {
        return match crate::function::BlastRadius::from_str(&v) {
            Some(tier) => AuthorityCeiling::UpTo(tier),
            None => AuthorityCeiling::MINIMAL, // fail safe on garbage
        };
    }
    AuthorityCeiling::UpTo(config.read().safety.default_ceiling)
}

/// Look up the tool's static tier and proven-reversibility from config.
///
/// Returns `(StaticTier, proven_reversible)`. Unknown tools (e.g. MCP, not in the
/// function set) resolve to `Unclassified` — the most conservative disposition.
fn tool_tier_and_reversibility(
    config: &GlobalConfig,
    tool_name: &str,
) -> (crate::function::StaticTier, bool) {
    use crate::function::StaticTier;
    let config_read = config.read();
    let decl = config_read
        .agent
        .as_ref()
        .and_then(|a| a.functions().find(tool_name))
        .or_else(|| config_read.functions.find(tool_name));
    match decl {
        // #6b consumes only *declared* intrinsic reversibility; registered
        // rollback artifacts (arg `false` here) arrive with #9/#10.
        Some(d) => (d.static_tier(), crate::safety::proven_reversible(d, false)),
        None => (StaticTier::Unclassified, false),
    }
}

/// Find a tool declaration from agent functions or global functions.
fn find_tool_declaration(
    config: &GlobalConfig,
    tool_name: &str,
) -> Option<crate::function::FunctionDeclaration> {
    let config_read = config.read();
    config_read
        .agent
        .as_ref()
        .and_then(|a| a.functions().find(tool_name))
        .or_else(|| config_read.functions.find(tool_name))
        .cloned()
}

/// Resolve the underlying script implementation or execution metadata for a tool.
fn resolve_tool_implementation(
    config: &GlobalConfig,
    tool_name: &str,
    agent_hint: Option<&str>,
) -> crate::safety::ToolImplementation {
    use crate::safety::ToolImplementation;

    #[cfg(feature = "mcp")]
    {
        let mcp_info = {
            let config_read = config.read();
            config_read
                .mcp_tools
                .get(tool_name)
                .map(|e| (e.server_name.clone(), e.original_name.clone()))
        };
        if let Some((server, tool)) = mcp_info {
            return ToolImplementation::Mcp { server, tool };
        }
    }

    let mut candidate_paths: Vec<std::path::PathBuf> = Vec::new();
    let config_read = config.read();

    let mut agents_to_check: Vec<String> = Vec::new();
    if let Some(agent) = agent_hint {
        agents_to_check.push(agent.to_string());
    }
    if let Some(agent) = &config_read.agent {
        let name = agent.name().to_string();
        if !agents_to_check.contains(&name) {
            agents_to_check.push(name);
        }
    }

    // Agent functions directory (including multi-tool scripts like tools.sh)
    for agent_name in agents_to_check {
        let agent_dir = Config::agent_functions_dir(&agent_name);
        for ext in &["sh", "bash", "nu", "py", "js"] {
            candidate_paths.push(agent_dir.join(format!("tools.{ext}")));
        }
        let agent_tools_dir = agent_dir.join("tools");
        if agent_tools_dir.exists() {
            for ext in &["sh", "py", "js", "bash", "nu", "rb"] {
                candidate_paths.push(agent_tools_dir.join(format!("{tool_name}.{ext}")));
            }
            candidate_paths.push(agent_tools_dir.join(tool_name));
        }
        let agent_bin_file = agent_dir.join("bin").join(tool_name);
        if agent_bin_file.exists() {
            candidate_paths.push(agent_bin_file);
        }
    }

    // Check all subdirectories in agents_functions_dir
    let agents_dir = Config::agents_functions_dir();
    if agents_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&agents_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    for ext in &["sh", "bash", "nu", "py", "js"] {
                        candidate_paths.push(p.join(format!("tools.{ext}")));
                    }
                }
            }
        }
    }

    // Global functions tools directory & multi-tool scripts
    let fn_dir = Config::functions_dir();
    for ext in &["sh", "bash", "nu", "py", "js"] {
        candidate_paths.push(fn_dir.join(format!("tools.{ext}")));
    }
    let functions_tools_dir = fn_dir.join("tools");
    if functions_tools_dir.exists() {
        for ext in &["sh", "py", "js", "bash", "nu", "rb"] {
            candidate_paths.push(functions_tools_dir.join(format!("{tool_name}.{ext}")));
        }
        candidate_paths.push(functions_tools_dir.join(tool_name));
    }

    // Global functions bin directory
    let functions_bin_file = Config::functions_bin_dir().join(tool_name);
    if functions_bin_file.exists() {
        candidate_paths.push(functions_bin_file);
    }
    drop(config_read);

    for path in candidate_paths {
        if !path.exists() {
            continue;
        }

        let resolved_path = if path.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                let target_str = target.to_string_lossy();
                if target_str.contains("run-tool.") {
                    if let Some(parent) = path.parent().and_then(|p| p.parent()) {
                        let tools_dir = parent.join("tools");
                        let mut found = None;
                        for ext in &["sh", "py", "js", "bash", "nu", "rb"] {
                            let p = tools_dir.join(format!("{tool_name}.{ext}"));
                            if p.exists() {
                                found = Some(p);
                                break;
                            }
                        }
                        found.unwrap_or(path)
                    } else {
                        path
                    }
                } else if let Ok(canonical) = path.canonicalize() {
                    canonical
                } else {
                    path
                }
            } else {
                path
            }
        } else {
            path
        };

        if !resolved_path.is_file() {
            continue;
        }

        const BUDGET: usize = 4096;
        match crate::safety::read_text_file_bounded(&resolved_path, BUDGET) {
            Ok(Some((text, truncated))) => {
                let is_multi_tool = resolved_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s == "tools")
                    .unwrap_or(false);

                if is_multi_tool {
                    if let Some(func_source) = crate::safety::extract_shell_function(&text, tool_name) {
                        let language = match resolved_path.extension().and_then(|e| e.to_str()) {
                            Some("sh" | "bash") => "bash".to_string(),
                            Some("py") => "python".to_string(),
                            Some("js") => "javascript".to_string(),
                            Some("nu") => "nushell".to_string(),
                            Some(other) => other.to_string(),
                            None => "text".to_string(),
                        };
                        return ToolImplementation::Script {
                            path: resolved_path.display().to_string(),
                            language,
                            source: func_source,
                            truncated: false,
                        };
                    } else {
                        // Function not in this multi-tool script; continue searching candidates
                        continue;
                    }
                }

                let language = match resolved_path.extension().and_then(|e| e.to_str()) {
                    Some("sh" | "bash") => "bash".to_string(),
                    Some("py") => "python".to_string(),
                    Some("js") => "javascript".to_string(),
                    Some("nu") => "nushell".to_string(),
                    Some(other) => other.to_string(),
                    None => "text".to_string(),
                };

                return ToolImplementation::Script {
                    path: resolved_path.display().to_string(),
                    language,
                    source: text,
                    truncated,
                };
            }
            Ok(None) => {
                return ToolImplementation::Binary {
                    path: resolved_path.display().to_string(),
                };
            }
            Err(_) => continue,
        }
    }

    ToolImplementation::Unknown
}

/// Format a human-readable preview of the tool invocation command.
fn format_tool_invocation(tool_name: &str, arguments: &serde_json::Value) -> Option<String> {
    match arguments {
        serde_json::Value::Object(map) if map.is_empty() => Some(tool_name.to_string()),
        serde_json::Value::Object(map) => {
            let mut parts = vec![tool_name.to_string()];
            for (k, v) in map {
                let flag = k.replace('_', "-");
                match v {
                    serde_json::Value::String(s) => {
                        parts.push(format!("--{flag} {:?}", s));
                    }
                    serde_json::Value::Bool(true) => {
                        parts.push(format!("--{flag}"));
                    }
                    serde_json::Value::Bool(false) => {}
                    _ => {
                        parts.push(format!("--{flag} {}", v));
                    }
                }
            }
            Some(parts.join(" "))
        }
        serde_json::Value::String(s) => {
            if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(s) {
                format_tool_invocation(tool_name, &serde_json::Value::Object(map))
            } else {
                Some(format!("{} {:?}", tool_name, s))
            }
        }
        serde_json::Value::Null => Some(tool_name.to_string()),
        _ => Some(format!("{} {}", tool_name, arguments)),
    }
}

/// Collect the string-valued arguments of a call, for policy matching.
fn string_args_from_value(args: &serde_json::Value) -> Vec<String> {
    let mut out = vec![];
    match args {
        serde_json::Value::Object(map) => {
            for v in map.values() {
                if let Some(s) = v.as_str() {
                    out.push(s.to_string());
                }
            }
        }
        serde_json::Value::String(s) => {
            // Arguments may arrive as a JSON string; try to parse and recurse,
            // else treat the whole string as one arg.
            if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(s)
            {
                for v in map.values() {
                    if let Some(vs) = v.as_str() {
                        out.push(vs.to_string());
                    }
                }
            } else {
                out.push(s.clone());
            }
        }
        _ => {}
    }
    out
}

fn string_args_of(call: &ToolCall) -> Vec<String> {
    string_args_from_value(&call.arguments)
}

/// Whether a tool call targets a sub-agent (agent-flagged function that names a
/// real agent), i.e. it is a delegation rather than a direct tool actuation.
///
/// Backlog #6b / decision (B): delegating to a sub-agent is *orchestration*, not
/// actuation — the real actuation risk is what the sub-agent *does*, which is
/// gated inside the child's own process (its inherited capability mask + authority
/// ceiling). So the delegation call itself is not subject to the authority gate.
fn call_targets_agent(config: &GlobalConfig, tool_name: &str) -> bool {
    let config_read = config.read();
    let has_agent_flag = if let Some(agent) = &config_read.agent {
        agent
            .functions()
            .find(tool_name)
            .is_some_and(|f| f.agent)
    } else {
        config_read
            .functions
            .find(tool_name)
            .is_some_and(|f| f.agent)
    };
    has_agent_flag && crate::config::list_agents().contains(&tool_name.to_string())
}

/// Formats the risk token on the LHS of the governance comparison:
/// - Unmodified: `risk safe`, `risk disruptive`
/// - Intrinsic reversibility: `risk disruptive (effective, reversible tool)`
/// - Preflight backup discount: `risk reversible (effective, via backup)`
/// - Policy raise: `risk destructive (effective, policy raise)`
/// - Unclassified: `risk human (unclassified tool)`
/// - Human required: `risk human (human approval required)`
pub fn format_risk_token(
    static_tier: crate::function::StaticTier,
    required: crate::safety::RequiredAuthority,
    mechanism: Option<&str>,
) -> String {
    use crate::safety::RequiredAuthority;
    match (required, static_tier) {
        (RequiredAuthority::Human, crate::function::StaticTier::Unclassified) => {
            "risk human (unclassified tool)".to_string()
        }
        (RequiredAuthority::Human, _) => {
            if let Some(m) = mechanism {
                format!("risk human (effective, {m})")
            } else {
                "risk human (human approval required)".to_string()
            }
        }
        (RequiredAuthority::Tier(rt), crate::function::StaticTier::Tier(st)) => {
            if rt < st {
                let mech_desc = mechanism.unwrap_or("reversible tool");
                format!("risk {} (effective, {})", rt.as_str(), mech_desc)
            } else if rt > st {
                format!("risk {} (effective, policy raise)", rt.as_str())
            } else {
                format!("risk {}", rt.as_str())
            }
        }
        (RequiredAuthority::Tier(rt), crate::function::StaticTier::Unclassified) => {
            format!("risk {} (effective, unclassified tool)", rt.as_str())
        }
    }
}

/// If the blast-radius authority ceiling (or the Protected Policy File) forbids
/// this tool call, return the structured denial result (backlog #6b).
///
/// Deterministic, no LLM. Returns `None` when the action is within the current
/// ceiling. Returns `Some(error_json)`:
/// - `policy_forbidden` — the policy file forbids the action outright;
/// - `authority_exceeded` — the required authority exceeds this agent's ceiling
///   (includes unclassified/human-reserved actions; pre-#6d these block rather
///   than escalate).
///
/// The `_plan` pseudo-tool is always permitted.
fn authority_denied_result(
    config: &GlobalConfig,
    call: &ToolCall,
    progress: Option<&AgentLoopProgress>,
    proven_reversible_applied: Option<&mut bool>,
) -> Option<serde_json::Value> {
    use crate::safety::{required_authority, PolicyFile, PolicyOutcome, RequiredAuthority};

    if call.name == "_plan" {
        return None;
    }

    // Decision (B): delegating to a sub-agent is orchestration, not actuation —
    // do not gate the delegation itself. The sub-agent's own actions are gated
    // inside its process (capability mask + authority ceiling, propagated via env).
    if call_targets_agent(config, &call.name) {
        return None;
    }

    // Load the Protected Policy File (absent → empty; owner-only enforced on load).
    let policy_path = config.read().safety.policy_file.clone();
    let policy = match policy_path {
        Some(p) => PolicyFile::load(&p).unwrap_or_default(),
        None => PolicyFile::default(),
    };
    let args: Vec<String> = string_args_of(call);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let policy_outcome = policy.evaluate(&call.name, &arg_refs);

    if let Some(outcome) = policy_outcome {
        if let Some(p) = progress {
            let outcome_str = match outcome {
                PolicyOutcome::Forbid => "forbid".to_string(),
                PolicyOutcome::Raise(t) => format!("raise to {}", t.as_str()),
            };
            p.emit(AgentLoopEvent::PolicyRuleMatched {
                name: call.name.clone(),
                outcome: outcome_str,
            });
        }
    }

    let (static_tier, intrinsic_reversible) = tool_tier_and_reversibility(config, &call.name);
    let is_proven_backup = proven_reversible_applied.as_ref().map(|b| **b).unwrap_or(false);
    let reversible = intrinsic_reversible || is_proven_backup;
    let base_mechanism: Option<&'static str> = if is_proven_backup {
        Some("via backup")
    } else if intrinsic_reversible {
        Some("reversible tool")
    } else {
        None
    };

    let mut required = required_authority(static_tier, policy_outcome, reversible);
    let ceiling = current_authority_ceiling(config);

    if ceiling.permits(required) {
        if let Some(p) = progress {
            let risk_token = format_risk_token(static_tier, required, base_mechanism);
            let comparison = format!("{} <= ceiling {}", risk_token, ceiling.tier().as_str());
            p.emit(AgentLoopEvent::SafetyGatePassed {
                name: call.name.clone(),
                comparison,
            });
        }
        return None;
    }

    // Opportunistic Remediation (Option B):
    // If the tool tripped the ceiling solely because it is not yet proven reversible,
    // check if it declares support for reversibility (e.g. `reversible-via backup`).
    // If taking an atomic backup drops required authority within ceiling, perform
    // the preflight backup right now to remediate and pass the gate!
    if !reversible && policy_outcome != Some(PolicyOutcome::Forbid) {
        let decl = find_tool_declaration(config, &call.name);
        let can_be_reversible = decl.as_ref().map(|d| {
            d.reversible_via.as_deref() == Some("backup") || d.reversible == Some(true)
        }).unwrap_or(false);

        if can_be_reversible {
            let remediated_required = required_authority(static_tier, policy_outcome, true);
            if ceiling.permits(remediated_required) {
                if let Some(_entry_id) = record_pre_mutation_journal_entry(config, call, progress) {
                    if let Some(flag) = proven_reversible_applied {
                        *flag = true;
                    }
                    required = remediated_required;

                    if let Some(p) = progress {
                        let step_str = match required {
                            RequiredAuthority::Tier(t) => t.as_str().to_string(),
                            RequiredAuthority::Human => "human".to_string(),
                        };
                        p.emit(AgentLoopEvent::PreflightReversibilityApplied {
                            name: call.name.clone(),
                            mechanism: "backup".to_string(),
                            stepped_down_to: step_str,
                        });
                        let risk_token = format_risk_token(static_tier, required, Some("via backup"));
                        let comparison = format!("{} <= ceiling {}", risk_token, ceiling.tier().as_str());
                        p.emit(AgentLoopEvent::SafetyGatePassed {
                            name: call.name.clone(),
                            comparison,
                        });
                    }
                    return None;
                }
            }
        }
    }

    // Distinguish an explicit policy forbid from a plain over-ceiling block.
    if policy_outcome == Some(PolicyOutcome::Forbid) {
        return Some(json!({
            "error": {
                "type": "policy_forbidden",
                "message": format!(
                    "Tool '{}' is forbidden by the protected safety policy. \
                     This is a non-pardonable deterministic rule and cannot be overridden.",
                    call.name
                )
            }
        }));
    }

    let required_desc = match required {
        RequiredAuthority::Human => "human approval".to_string(),
        RequiredAuthority::Tier(t) => format!("'{}' authority", t.as_str()),
    };
    let risk_token = format_risk_token(static_tier, required, base_mechanism);
    let comparison = format!("{} > ceiling {}", risk_token, ceiling.tier().as_str());
    Some(json!({
        "error": {
            "type": "authority_exceeded",
            "comparison": comparison,
            "message": format!(
                "Tool '{}' requires {} which exceeds this agent's authority ceiling ('{}'). \
                 Return findings to your caller so a higher-authority agent (or a human) can actuate.",
                call.name, required_desc, ceiling.tier().as_str()
            )
        }
    }))
}

/// Extract the safety-gate denial reason from a tool result, if it is one.
///
/// The #6a capability gate and the #6b authority/policy gate both return their
/// refusals as structured `Ok` values (so the model sees them verbatim) — they
/// are NOT executions. This inspects the result's `error.type` for the gate
/// reasons so the dispatcher can emit `ToolBlocked` instead of `ToolComplete`.
fn safety_block_reason(value: &serde_json::Value) -> Option<String> {
    let err = value.get("error")?;
    let err_type = err.get("type")?.as_str()?;
    match err_type {
        "authority_exceeded" | "risk_blocked" => {
            if let Some(cmp) = err.get("comparison").and_then(|v| v.as_str()) {
                Some(cmp.to_string())
            } else {
                Some(err_type.to_string())
            }
        }
        "capability_denied"
        | "policy_forbidden"
        | "escalation_halted"
        | "escalation_reverted"
        | "escalation_failed" => Some(err_type.to_string()),
        _ => None,
    }
}

/// Backlog #6c: turn a (clamped) risk decision into a structured denial, if any.
///
/// **Pure and deterministic** — this is the unit-testable heart of the #6c
/// overlay; the live model call ([`run_risk_evaluator`]) is a thin wrapper that
/// feeds this. Given the deterministic base authority, this agent's ceiling, and
/// a parsed [`RiskVerdict`], it:
///   1. applies the stricter-only clamp (verdict may only raise), then
///   2. **fails toward blocking**: a `Low`-confidence verdict is treated as an
///      escalation trigger (pre-#6d: a block), even if the clamped tier still
///      fits the ceiling — an untrustworthy judgment must not green-light an
///      already-risky (non-`Safe`) action, and
///   3. blocks when the clamped authority exceeds the ceiling.
///
/// Returns `Some(error_json)` with `type: "risk_blocked"` when the action must be
/// stopped, or `None` when it may proceed. The caller only invokes this for
/// non-`Safe` actions (the `Safe` fast-path skips the evaluator entirely), so a
/// low-confidence block here never affects reads.
fn risk_denied_from_verdict(
    tool_name: &str,
    base: crate::safety::RequiredAuthority,
    ceiling: crate::safety::AuthorityCeiling,
    verdict: &crate::safety::RiskVerdict,
    reversible: bool,
) -> Option<serde_json::Value> {
    use crate::safety::{clamp_verdict, VerdictConfidence};

    let effective = clamp_verdict(base, verdict, reversible);
    let low_confidence = verdict.confidence == VerdictConfidence::Low;

    if ceiling.permits(effective) && !low_confidence {
        return None;
    }

    let comparison = if low_confidence && ceiling.permits(effective) {
        let eff_str = match effective {
            crate::safety::RequiredAuthority::Human => "human",
            crate::safety::RequiredAuthority::Tier(t) => t.as_str(),
        };
        format!("risk evaluator low confidence ({eff_str} requires higher confidence)")
    } else {
        let eff_str = match effective {
            crate::safety::RequiredAuthority::Human => "human (human approval required)".to_string(),
            crate::safety::RequiredAuthority::Tier(t) => format!("{} (effective, evaluator raise)", t.as_str()),
        };
        format!(
            "risk {} > ceiling {}",
            eff_str,
            ceiling.tier().as_str()
        )
    };

    let detail = if low_confidence && ceiling.permits(effective) {
        format!(
            "the risk evaluator returned low confidence for tool '{tool_name}', so it \
             cannot be autonomously authorized"
        )
    } else {
        format!(
            "the risk evaluator raised tool '{}' to a level exceeding this agent's \
             authority ceiling ('{}')",
            tool_name,
            ceiling.tier().as_str()
        )
    };
    let rationale = if verdict.rationale.is_empty() {
        String::new()
    } else {
        format!(" Evaluator rationale: {}.", verdict.rationale)
    };
    Some(json!({
        "error": {
            "type": "risk_blocked",
            "comparison": comparison,
            "message": format!(
                "Blocked by the risk evaluator: {detail}.{rationale} \
                 Return findings to your caller so a higher-authority agent (or a human) can decide."
            )
        }
    }))
}

/// Backlog #6c: run the `%assess-risk%` evaluator for a single action and return
/// a structured denial if it must be blocked.
///
/// Returns `None` (proceed) when: `safety.risk_model` is unset (degrade to #6b);
/// the static tier is `Safe` (fast-path); the tool is `_plan` / a delegation; the
/// base authority is already `Human` (deterministically escalates); or the verdict
/// clamps within ceiling with adequate confidence. Any model/error is treated as a
/// low-confidence verdict (fail-toward), NEVER as a pass.
async fn risk_evaluator_denied_result(
    config: &GlobalConfig,
    call: &ToolCall,
    proven_reversible: bool,
    cache: Option<&std::sync::Arc<parking_lot::Mutex<crate::safety::RiskCache>>>,
    progress: Option<&AgentLoopProgress>,
) -> Option<serde_json::Value> {
    use crate::safety::{
        build_evaluator_context, clamp_verdict, required_authority, PolicyFile, RequiredAuthority,
        RiskVerdict,
    };

    let risk_model = config
        .read()
        .safety
        .risk_model
        .clone()
        .or_else(|| {
            config
                .read()
                .role_model_id(crate::config::ASSESS_RISK_ROLE)
        })?;

    if call.name == "_plan" {
        return None;
    }
    if call_targets_agent(config, &call.name) {
        return None;
    }

    let (static_tier, static_reversible) = tool_tier_and_reversibility(config, &call.name);
    let reversible = static_reversible || proven_reversible;
    let base_tier = match static_tier {
        crate::function::StaticTier::Tier(t) => t,
        crate::function::StaticTier::Unclassified => return None,
    };

    // Fast-path (FR-6c.6): Safe actions (reads) never consult the evaluator.
    if base_tier == crate::function::BlastRadius::Safe {
        return None;
    }

    let policy_path = config.read().safety.policy_file.clone();
    let policy = match policy_path {
        Some(p) => PolicyFile::load(&p).unwrap_or_default(),
        None => PolicyFile::default(),
    };
    let args: Vec<String> = string_args_of(call);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let policy_outcome = policy.evaluate(&call.name, &arg_refs);
    let base = required_authority(static_tier, policy_outcome, reversible);
    if base == RequiredAuthority::Human {
        return None;
    }
    let ceiling = current_authority_ceiling(config);

    // #6c two-phase (FR-6c.7, FR-6c.9): consult the monotonic, raise-only cache first.
    // By caching RiskVerdict instead of scalar authority, we preserve evaluator rationale
    // and dynamically re-clamp against act-time reversibility.
    if let Some(cache) = cache {
        if let Some(cached_verdict) = cache.lock().get(&call.name, &call.arguments) {
            let clamped = clamp_verdict(base, &cached_verdict, reversible);
            let effective = if cached_verdict.confidence == crate::safety::VerdictConfidence::Low {
                RequiredAuthority::Human
            } else {
                clamped
            };
            let floor_str = match effective {
                RequiredAuthority::Human => "human".to_string(),
                RequiredAuthority::Tier(t) => t.as_str().to_string(),
            };
            if let Some(p) = progress {
                p.emit(AgentLoopEvent::RiskAssessmentCacheHit {
                    name: call.name.clone(),
                    cached_floor: floor_str,
                    rationale: if cached_verdict.rationale.is_empty() {
                        None
                    } else {
                        Some(cached_verdict.rationale.clone())
                    },
                });
            }
            if ceiling.permits(effective)
                && cached_verdict.confidence != crate::safety::VerdictConfidence::Low
            {
                return None;
            }
            return risk_blocked_result(&call.name, ceiling, Some(&cached_verdict.rationale));
        }
    }

    // Cache miss → run the evaluator. Any model/parse error → low-confidence
    // fallback (fail-toward), never a silent pass.
    let intent = format!("execute tool '{}'", call.name);
    let decl = find_tool_declaration(config, &call.name);
    let agent_name = config.read().agent.as_ref().map(|a| a.name().to_string());
    let impl_info = resolve_tool_implementation(config, &call.name, agent_name.as_deref());
    let decl_ctx = crate::safety::extract_declaration_context(decl.as_ref(), impl_info.source());
    let invocation = format_tool_invocation(&call.name, &call.arguments);
    let fn_dir = Config::functions_dir();
    let helpers = if let Some(source) = impl_info.source() {
        crate::safety::resolve_tool_helpers(&fn_dir, source)
    } else {
        Vec::new()
    };
    let context = build_evaluator_context(
        &call.name,
        &call.arguments,
        reversible,
        &intent,
        decl_ctx.as_ref(),
        Some(&impl_info),
        invocation.as_deref(),
        if helpers.is_empty() { None } else { Some(&helpers) },
    );

    if let Some(p) = progress {
        p.emit(AgentLoopEvent::RiskAssessmentStart {
            name: call.name.clone(),
            model: risk_model.clone(),
        });
    }

    let verdict = match run_risk_evaluator(config, &risk_model, &context, progress).await {
        Ok(raw) => {
            let v = RiskVerdict::parse(&raw, base_tier);
            if let Some(p) = progress {
                p.emit(AgentLoopEvent::RiskAssessmentComplete {
                    name: call.name.clone(),
                    tier: v.tier.as_str().to_string(),
                    confidence: format!("{:?}", v.confidence),
                    rationale: v.rationale.clone(),
                });
            }
            v
        }
        Err(err) => {
            if let Some(p) = progress {
                p.emit(AgentLoopEvent::RiskAssessmentError {
                    name: call.name.clone(),
                    error: err.to_string(),
                });
            }
            RiskVerdict::low_confidence_fallback(base_tier)
        }
    };

    if let Some(cache) = cache {
        cache.lock().raise(&call.name, &call.arguments, verdict.clone());
    }

    risk_denied_from_verdict(&call.name, base, ceiling, &verdict, reversible)
}

/// Build the structured `risk_blocked` denial for a cache-driven block (no live
/// verdict rationale available). Kept consistent with [`risk_denied_from_verdict`].
fn risk_blocked_result(
    tool_name: &str,
    ceiling: crate::safety::AuthorityCeiling,
    rationale: Option<&str>,
) -> Option<serde_json::Value> {
    let rationale = rationale
        .filter(|r| !r.is_empty())
        .map(|r| format!(" Evaluator rationale: {r}."))
        .unwrap_or_default();
    Some(json!({
        "error": {
            "type": "risk_blocked",
            "message": format!(
                "Blocked by the risk evaluator: tool '{}' was assessed at a level exceeding \
                 this agent's authority ceiling ('{}').{} \
                 Return findings to your caller so a higher-authority agent (or a human) can decide.",
                tool_name, ceiling.tier().as_str(), rationale
            )
        }
    }))
}

/// Invoke the `%assess-risk%` role with the dedicated evaluator model, returning
/// the raw model text. Isolated so the surrounding logic stays offline-testable.
async fn run_risk_evaluator(
    config: &GlobalConfig,
    risk_model: &str,
    context: &str,
    progress: Option<&AgentLoopProgress>,
) -> Result<String> {
    use crate::client::{Model, ModelType};
    use crate::config::{Input, RoleLike, ASSESS_RISK_ROLE};

    let mut role = config.read().retrieve_role(ASSESS_RISK_ROLE)?;
    if role.model_id() != Some(risk_model) {
        let model = Model::retrieve_model(&config.read(), risk_model, ModelType::Chat)?;
        role.set_model(model);
    }

    let input = Input::from_str(config, context, Some(role.to_role()));
    let show_dialog = config.read().agent_loop.show_dialog;
    let no_truncate = config.read().agent_loop.dialog_no_truncate;
    let pid = std::process::id();
    if show_dialog {
        let prompt_display = match input.build_messages() {
            Ok(msgs) => format_messages_dialog(&msgs, no_truncate),
            Err(_) => context.to_string(),
        };
        if let Some(p) = progress {
            p.emit(AgentLoopEvent::DialogBlock {
                agent: ASSESS_RISK_ROLE.to_string(),
                pid,
                turn: 1,
                max_turns: 1,
                direction: DialogDirection::Request,
                content: prompt_display,
            });
        } else {
            emit_dialog_block(
                ASSESS_RISK_ROLE,
                pid,
                1,
                1,
                DialogDirection::Request,
                &prompt_display,
            );
        }
    }
    let res = input.fetch_chat_text().await;
    match &res {
        Ok(text) => {
            if show_dialog {
                let response_content = truncate_payload_dialog(text, 20, 20, no_truncate);
                if let Some(p) = progress {
                    p.emit(AgentLoopEvent::DialogBlock {
                        agent: ASSESS_RISK_ROLE.to_string(),
                        pid,
                        turn: 1,
                        max_turns: 1,
                        direction: DialogDirection::Response,
                        content: response_content.clone(),
                    });
                } else {
                    emit_dialog_block(
                        ASSESS_RISK_ROLE,
                        pid,
                        1,
                        1,
                        DialogDirection::Response,
                        &response_content,
                    );
                }
            }
        }
        Err(err) => {
            if show_dialog {
                let err_msg = format!("(LLM error: {err})");
                if let Some(p) = progress {
                    p.emit(AgentLoopEvent::DialogBlock {
                        agent: ASSESS_RISK_ROLE.to_string(),
                        pid,
                        turn: 1,
                        max_turns: 1,
                        direction: DialogDirection::Response,
                        content: err_msg,
                    });
                } else {
                    emit_dialog_block(
                        ASSESS_RISK_ROLE,
                        pid,
                        1,
                        1,
                        DialogDirection::Response,
                        &err_msg,
                    );
                }
            }
        }
    }
    res
}

/// Prompt the human operator when an action requiring Human authority reaches the root orchestrator (FR-6d.8).
fn prompt_human_verdict(
    tool_name: &str,
    arguments: &serde_json::Value,
    blast_radius: crate::function::BlastRadius,
    reason: &str,
    ceiling: crate::safety::AuthorityCeiling,
    progress: Option<&AgentLoopProgress>,
) -> Result<crate::safety::VerdictDecision> {
    use crate::safety::VerdictDecision;

    if let Some(p) = progress {
        p.emit(AgentLoopEvent::HumanPromptRequested {
            name: tool_name.to_string(),
            blast_radius: blast_radius.as_str().to_string(),
            reason: reason.to_string(),
        });
    }

    if !*IS_STDOUT_TERMINAL {
        // Headless mode: emit structured JSON and fail-closed
        if let Some(p) = progress {
            p.emit(AgentLoopEvent::HumanVerdictReceived {
                name: tool_name.to_string(),
                decision: "Halt".to_string(),
            });
        }
        eprintln!(
            "{}",
            serde_json::json!({
                "event": "escalation_headless_denied",
                "tool": tool_name,
                "arguments": arguments,
                "blast_radius": blast_radius.as_str(),
                "reason": reason,
            })
        );
        return Ok(VerdictDecision::Halt);
    }

    let blocked_summary = format!("risk {} > ceiling {}", blast_radius.as_str(), ceiling.tier().as_str());
    let banner = color_text(
        &format!(
            "\n[HUMAN APPROVAL REQUIRED] {tool_name}\n  risk:    {} (tool)\n  ceiling: {} (agent)\n  blocked: {blocked_summary}\n  args:    {arguments}\n  reason:  {reason}",
            blast_radius.as_str(),
            ceiling.tier().as_str(),
        ),
        nu_ansi_term::Color::Yellow,
    );
    println!("{banner}");

    let options = ["continue", "halt", "revert", "explain", "guide"];
    let first_letter_color = nu_ansi_term::Color::Cyan;
    let prompt_text = options
        .iter()
        .map(|v| format!("{}{}", color_text(&v[0..1], first_letter_color), &v[1..]))
        .collect::<Vec<String>>()
        .join(&color_text(" | ", nu_ansi_term::Color::DarkGray));

    loop {
        let answer_char = crate::utils::read_single_key(
            &['c', 'h', 'r', 'e', 'g'],
            'h',
            &format!("{prompt_text}: "),
        )?;

        let decision = match answer_char {
            'c' => VerdictDecision::Continue,
            'h' => VerdictDecision::Halt,
            'r' => VerdictDecision::Revert,
            'e' => {
                println!(
                    "{}",
                    color_text(
                        &format!(
                            "Action details:\n  Tool: {tool_name}\n  Tier: {}\n  Arguments: {arguments:#}\n  Rationale: {reason}",
                            blast_radius.as_str()
                        ),
                        nu_ansi_term::Color::LightBlue,
                    )
                );
                continue;
            }
            'g' => {
                let guidance = inquire::Text::new("Enter instructions/guidance for the agent:").prompt()?;
                println!("Guidance recorded: {guidance}");
                VerdictDecision::Halt
            }
            _ => VerdictDecision::Halt,
        };

        if let Some(p) = progress {
            p.emit(AgentLoopEvent::HumanVerdictReceived {
                name: tool_name.to_string(),
                decision: format!("{decision:?}"),
            });
        }
        return Ok(decision);
    }
}

static CHILD_CLIENT: tokio::sync::OnceCell<Option<Arc<crate::escalation::ChildEscalationClient>>> =
    tokio::sync::OnceCell::const_new();

pub async fn get_or_init_child_client() -> Option<Arc<crate::escalation::ChildEscalationClient>> {
    let client = CHILD_CLIENT
        .get_or_init(|| async {
            if let Some(parent_info) = crate::escalation::ParentConnInfo::from_env() {
                let depth = std::env::var("AICHAT_AGENT_DEPTH")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let agent_id = std::env::var("AICHAT_AGENT_NAME")
                    .unwrap_or_else(|_| format!("agent-d{depth}"));
                match crate::escalation::ChildEscalationClient::connect(&parent_info, &agent_id, depth).await {
                    Ok(c) => Some(c),
                    Err(e) => {
                        log::warn!("Failed to initialize persistent child escalation client: {e}");
                        None
                    }
                }
            } else {
                None
            }
        })
        .await;
    client.clone()
}

static TREE_LISTENER: tokio::sync::OnceCell<Arc<crate::escalation::ParentListener>> =
    tokio::sync::OnceCell::const_new();

async fn get_or_init_parent_listener(
    config: &GlobalConfig,
) -> Result<Option<Arc<crate::escalation::ParentListener>>> {
    let listener = TREE_LISTENER
        .get_or_try_init(|| async {
            let tree_id = std::env::var("AICHAT_TREE_ID")
                .unwrap_or_else(|_| format!("tree-{}", uuid::Uuid::new_v4()));
            let tree_secret = std::env::var("AICHAT_TREE_SECRET").unwrap_or_else(|_| {
                crate::utils::sha256_bytes(format!("{}-{}", std::process::id(), uuid::Uuid::new_v4()).as_bytes())
            });
            let identity = Arc::new(crate::escalation::generate_tree_identity()?);
            let listener = Arc::new(
                crate::escalation::ParentListener::bind(identity, tree_id, tree_secret).await?,
            );

            // Spawn background accept loop for handling child escalations
            let listener_clone = listener.clone();
            let config_clone = config.clone();
            tokio::spawn(async move {
                while let Ok((mut transport, hello)) = listener_clone.accept_authenticated().await {
                    let config = config_clone.clone();
                    tokio::spawn(async move {
                        while let Ok(Some(msg)) = transport.recv_upstream().await {
                            match msg {
                                crate::safety::UpstreamMsg::Event(val) => {
                                    let show_trace = config.read().agent_loop.show_trace
                                        || config.read().multi_agent.show_trace;
                                    if show_trace {
                                        let summary = if let Some(ev) = val.get("event").and_then(|v| v.as_str()) {
                                            ev.to_string()
                                        } else {
                                            val.to_string()
                                        };
                                        eprintln!("  [child {}] {}", hello.agent_id, summary);
                                    }
                                }
                                crate::safety::UpstreamMsg::Escalation(esc) => {
                                    let verdict = handle_escalation_request(&config, &hello, esc).await;
                                    let _ = transport.send_downstream(&crate::safety::DownstreamMsg::Verdict(verdict)).await;
                                }
                                crate::safety::UpstreamMsg::Result(res) => {
                                    log::debug!("Child agent {} completed task with cost {}", hello.agent_id, res.cost);
                                    break;
                                }
                                crate::safety::UpstreamMsg::Error(err) => {
                                    log::debug!("Child agent {} failed task: {}", hello.agent_id, err.message);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    });
                }
            });

            Ok::<_, anyhow::Error>(listener)
        })
        .await?;
    Ok(Some(listener.clone()))
}

/// Backlog #6d / Option B: pure decision helper for supervisory escalation.
/// Combines the base required authority (from tool tier + policy + reversibility)
/// with an optional risk evaluator verdict under the supervisor's authority ceiling.
/// Low-confidence verdicts fail toward Human authority.
fn supervisory_verdict_decision(
    base: crate::safety::RequiredAuthority,
    ceiling: crate::safety::AuthorityCeiling,
    verdict: Option<&crate::safety::RiskVerdict>,
    reversible: bool,
) -> (crate::safety::RequiredAuthority, bool) {
    let effective = match verdict {
        Some(v) => {
            if v.confidence == crate::safety::VerdictConfidence::Low {
                crate::safety::RequiredAuthority::Human
            } else {
                crate::safety::clamp_verdict(base, v, reversible)
            }
        }
        None => base,
    };
    let permits = ceiling.permits(effective);
    (effective, permits)
}

async fn handle_escalation_request(
    config: &GlobalConfig,
    hello: &crate::safety::HelloMsg,
    esc: crate::safety::EscalationMsg,
) -> crate::safety::VerdictMsg {
    let escalation_id = esc.id.clone();
    let tool_name = esc.action.get("tool").and_then(|v| v.as_str()).unwrap_or("unknown");
    let args = esc.action.get("arguments").cloned().unwrap_or(serde_json::Value::Null);
    let show_trace = config.read().agent_loop.show_trace || config.read().multi_agent.show_trace;

    if show_trace {
        eprintln!(
            "  [supervisor] received escalation from child '{}' (depth {}) for tool '{}' (reported tier: {}, reversible: {})",
            hello.agent_id, hello.depth, tool_name, esc.blast_radius.as_str(), esc.reversible
        );
    }

    // Defense-in-depth (FR-6d.21 & FR-6d.24): neither capability mask nor authority ceiling
    // can be elevated in-flight over mTLS. Sub-agents must be re-delegated with upfront permissions.
    if esc.reason == "capability_denied" || esc.reason == "authority_exceeded" {
        if show_trace {
            eprintln!(
                "  [supervisor] rejected in-flight permission elevation ({}) for child '{}' tool '{}'",
                esc.reason, hello.agent_id, tool_name
            );
        }
        let msg = if esc.reason == "capability_denied" {
            "Capability mask is a hard process sandbox boundary and cannot be elevated in-flight. Re-delegate the sub-agent with an explicit mutating permission contract."
        } else {
            "Authority ceiling is a hard process sandbox boundary and cannot be elevated in-flight. Re-delegate the sub-agent with an explicit permission contract or execute directly."
        };
        return crate::safety::VerdictMsg {
            escalation_id,
            decision: crate::safety::VerdictDecision::Halt,
            added_context: Some(serde_json::json!({
                "error": {
                    "type": esc.reason,
                    "message": msg,
                }
            })),
        };
    }

    // 1. Protected Policy Check (Supervisor's own non-pardonable deterministic policy)
    let policy_path = config.read().safety.policy_file.clone();
    let policy = match policy_path {
        Some(p) => crate::safety::PolicyFile::load(&p).unwrap_or_default(),
        None => crate::safety::PolicyFile::default(),
    };
    let arg_strings = string_args_from_value(&args);
    let arg_refs: Vec<&str> = arg_strings.iter().map(|s| s.as_str()).collect();
    let policy_outcome = policy.evaluate(tool_name, &arg_refs);

    if policy_outcome == Some(crate::safety::PolicyOutcome::Forbid) {
        if show_trace {
            eprintln!(
                "  [supervisor] policy forbids tool '{}' requested by child '{}'",
                tool_name, hello.agent_id
            );
        }
        return crate::safety::VerdictMsg {
            escalation_id,
            decision: crate::safety::VerdictDecision::Halt,
            added_context: Some(serde_json::json!({
                "error": {
                    "type": "policy_forbidden",
                    "message": format!(
                        "Tool '{}' is forbidden by the supervisor's protected safety policy. \
                         This is a non-pardonable deterministic rule and cannot be overridden.",
                        tool_name
                    )
                }
            })),
        };
    }

    // 2. Determine static tier, reversibility, and base required authority
    let (supervisor_static_tier, static_reversible) = tool_tier_and_reversibility(config, tool_name);
    let effective_blast_radius = match supervisor_static_tier {
        crate::function::StaticTier::Tier(t) => t.max(esc.blast_radius),
        crate::function::StaticTier::Unclassified => esc.blast_radius,
    };
    let static_tier = crate::function::StaticTier::Tier(effective_blast_radius);
    let decl = find_tool_declaration(config, tool_name);
    let can_be_reversible = decl.as_ref().map(|d| {
        d.reversible_via.as_deref() == Some("backup") || d.reversible == Some(true)
    }).unwrap_or(static_reversible);
    let reversible = can_be_reversible && (static_reversible || esc.reversible);
    let base_required = crate::safety::required_authority(static_tier, policy_outcome, reversible);
    let current_ceiling = current_authority_ceiling(config);

    // 3. Supervisory Risk Evaluation ("The Should Gate")
    let risk_model = config
        .read()
        .safety
        .risk_model
        .clone()
        .or_else(|| {
            config
                .read()
                .role_model_id(crate::config::ASSESS_RISK_ROLE)
        });

    let mut evaluator_verdict: Option<crate::safety::RiskVerdict> = None;

    if effective_blast_radius != crate::function::BlastRadius::Safe {
        if let Some(ref rm) = risk_model {
            let impl_info = resolve_tool_implementation(config, tool_name, Some(&hello.agent_id));
            let decl_ctx = crate::safety::extract_declaration_context(decl.as_ref(), impl_info.source());
            let invocation = format_tool_invocation(tool_name, &args);
            let supervisory_intent = format!(
                "Supervisory authorization: child agent '{}' (depth {}) requested permission to execute tool '{}'. Child reason: '{}'",
                hello.agent_id, hello.depth, tool_name, esc.reason
            );
            let fn_dir = Config::functions_dir();
            let helpers = if let Some(source) = impl_info.source() {
                crate::safety::resolve_tool_helpers(&fn_dir, source)
            } else {
                Vec::new()
            };
            let eval_context_str = crate::safety::build_evaluator_context(
                tool_name,
                &args,
                reversible,
                &supervisory_intent,
                decl_ctx.as_ref(),
                Some(&impl_info),
                invocation.as_deref(),
                if helpers.is_empty() { None } else { Some(&helpers) },
            );

            if show_trace {
                eprintln!(
                    "  [supervisor] evaluating risk of child '{}' action '{}' with model '{}' ...",
                    hello.agent_id, tool_name, rm
                );
            }

            let verdict = match run_risk_evaluator(config, rm, &eval_context_str, None).await {
                Ok(raw) => {
                    let v = crate::safety::RiskVerdict::parse(&raw, effective_blast_radius);
                    if show_trace {
                        eprintln!(
                            "  [supervisor] risk assessment completed for '{}': tier={}, confidence={:?}, rationale={}",
                            tool_name, v.tier.as_str(), v.confidence, v.rationale
                        );
                    }
                    v
                }
                Err(err) => {
                    if show_trace {
                        eprintln!(
                            "  [supervisor] risk assessment error for '{}': {err} (falling back to low-confidence)",
                            tool_name
                        );
                    }
                    crate::safety::RiskVerdict::low_confidence_fallback(effective_blast_radius)
                }
            };
            evaluator_verdict = Some(verdict);
        }
    }

    let (required, permitted) = supervisory_verdict_decision(
        base_required,
        current_ceiling,
        evaluator_verdict.as_ref(),
        reversible,
    );

    // 4. Authority Decision: If permitted within ceiling, approve autonomously!
    if permitted {
        if show_trace {
            eprintln!(
                "  [supervisor] approving child '{}' escalation for '{}' (required: {:?}, ceiling: {:?})",
                hello.agent_id, tool_name, required, current_ceiling.tier()
            );
        }
        let added_context = evaluator_verdict.as_ref().map(|v| serde_json::json!({
            "supervisor": "approved",
            "evaluator_tier": v.tier.as_str(),
            "evaluator_rationale": v.rationale,
        }));
        return crate::safety::VerdictMsg {
            escalation_id,
            decision: crate::safety::VerdictDecision::Continue,
            added_context,
        };
    }

    if show_trace {
        eprintln!(
            "  [supervisor] escalation for tool '{}' requires {:?} exceeding ceiling {:?} -> escalating upward",
            tool_name, required, current_ceiling.tier()
        );
    }

    // 5. Over-ceiling: Re-escalate upward if parent exists (depth > 0)
    if let Some(parent_info) = crate::escalation::ParentConnInfo::from_env() {
        let current_depth: usize = std::env::var("AICHAT_AGENT_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let timeout_secs = config.read().safety.verdict_timeout_secs;
        if let Some(client) = get_or_init_child_client().await {
            match client.escalate(esc, timeout_secs).await {
                Ok(v) => return v,
                Err(e) => {
                    return crate::safety::VerdictMsg {
                        escalation_id,
                        decision: crate::safety::VerdictDecision::Halt,
                        added_context: Some(serde_json::json!({"error": format!("Re-escalation to parent failed: {e}")})),
                    };
                }
            }
        } else {
            match crate::escalation::escalate_to_parent(
                &parent_info,
                &hello.agent_id,
                current_depth,
                esc,
                timeout_secs,
            )
            .await
            {
                Ok(v) => return v,
                Err(_) => {
                    return crate::safety::VerdictMsg {
                        escalation_id,
                        decision: crate::safety::VerdictDecision::Halt,
                        added_context: None,
                    };
                }
            }
        }
    }

    // 6. Root orchestrator (Depth 0): Prompt Human in the loop!
    let prompt_reason = if let Some(ref v) = evaluator_verdict {
        if !v.rationale.is_empty() {
            format!("{} (Supervisor Evaluator Rationale: {})", esc.reason, v.rationale)
        } else {
            esc.reason.clone()
        }
    } else {
        esc.reason.clone()
    };

    let decision = prompt_human_verdict(tool_name, &args, effective_blast_radius, &prompt_reason, current_ceiling, None)
        .unwrap_or(crate::safety::VerdictDecision::Halt);

    crate::safety::VerdictMsg {
        escalation_id,
        decision,
        added_context: None,
    }
}

fn record_pre_mutation_journal_entry(
    config: &GlobalConfig,
    call: &ToolCall,
    progress: Option<&AgentLoopProgress>,
) -> Option<String> {
    let (static_tier, _) = tool_tier_and_reversibility(config, &call.name);
    if static_tier != crate::function::StaticTier::Tier(crate::function::BlastRadius::Safe) {
        let tree_id = std::env::var("AICHAT_TREE_ID").unwrap_or_else(|_| "tree-local".into());
        let agent_id = std::env::var("AICHAT_AGENT_NAME").unwrap_or_else(|_| "agent".into());
        let journal_dir = crate::safety::RollbackJournal::resolve_journal_dir(&config.read().safety.escalation_dir);
        if let Ok(journal) = crate::safety::RollbackJournal::open(&journal_dir, &tree_id, &agent_id) {
            let entry_id = format!("entry-{}", uuid::Uuid::new_v4());

            let mut target_path = None;
            let mut artifact_path = None;
            let mut undo_command = None;

            if let Some(obj) = call.arguments.as_object() {
                if let Some(path_str) = obj.get("path").or_else(|| obj.get("file")).and_then(|v| v.as_str()) {
                    let target = if Path::new(path_str).is_absolute() {
                        PathBuf::from(path_str)
                    } else {
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path_str)
                    };
                    target_path = Some(target.clone());
                    if target.exists() {
                        let backup_name = format!("{}-{}.bak", target.file_name().and_then(|n| n.to_str()).unwrap_or("file"), entry_id);
                        let backup_file = journal.backups_dir().join(backup_name);
                        if std::fs::copy(&target, &backup_file).is_ok() {
                            artifact_path = Some(backup_file);
                        }
                    } else {
                        undo_command = Some(format!("rm -f '{}'", target.display()));
                    }
                }
            }

            let entry = crate::safety::RollbackJournalEntry {
                id: entry_id.clone(),
                agent_id,
                tree_id,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                tool: call.name.clone(),
                args: call.arguments.clone(),
                working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                shell: Some("/bin/bash".into()),
                target_path,
                artifact_path,
                undo_command,
            };
            if journal.record(&entry).is_ok() {
                if let Some(p) = progress {
                    p.emit(AgentLoopEvent::RollbackJournalRecorded {
                        name: call.name.clone(),
                        entry_id: entry_id.clone(),
                        entry: Some(Box::new(entry)),
                    });
                }
                return Some(entry_id);
            }
        }
    }
    None
}

/// Dispatch a single tool call asynchronously.
async fn eval_single_tool(
    config: &GlobalConfig,
    call: &ToolCall,
    risk_cache: Option<&std::sync::Arc<parking_lot::Mutex<crate::safety::RiskCache>>>,
    progress: Option<&AgentLoopProgress>,
) -> Result<serde_json::Value> {
    // Backlog #6a: capability-mask gate — a hard process sandbox boundary.
    // NO in-flight escalation over mTLS (a mask is a static permission, not an authorization).
    // Return the denial immediately so the child loop can unwind and report `permission_blocked`.
    if let Some(denied) = capability_denied_result(config, &call.name) {
        return Ok(denied);
    }

    let mut proven_reversible_applied = false;
    // Backlog #6b: blast-radius authority gate — a hard process sandbox boundary for sub-agents (FR-6d.24).
    // Sub-agents CANNOT elevate authority ceiling in-flight over mTLS.
    // Return the denial immediately so the child loop can unwind and report `permission_blocked`.
    if let Some(denied) = authority_denied_result(config, call, progress, Some(&mut proven_reversible_applied)) {
        let err_type = denied
            .get("error")
            .and_then(|e| e.get("type"))
            .and_then(|t| t.as_str())
            .unwrap_or("authority_exceeded");
        let is_child = current_agent_depth() > 0 || crate::escalation::ParentConnInfo::from_env().is_some();
        if err_type == "policy_forbidden" || is_child {
            return Ok(denied);
        }

        let (static_tier, _) = tool_tier_and_reversibility(config, &call.name);
        let blast_radius = match static_tier {
            crate::function::StaticTier::Tier(t) => t,
            crate::function::StaticTier::Unclassified => crate::function::BlastRadius::Catastrophic,
        };
        if *IS_STDOUT_TERMINAL {
            let ceiling = current_authority_ceiling(config);
            let decision = prompt_human_verdict(&call.name, &call.arguments, blast_radius, "authority_exceeded", ceiling, progress)?;
            match decision {
                crate::safety::VerdictDecision::Continue => {}
                crate::safety::VerdictDecision::Halt => {
                    return Ok(json!({"error": {"type": "escalation_halted", "message": "Action halted by human operator"}}));
                }
                crate::safety::VerdictDecision::Revert => {
                    let tree_id = std::env::var("AICHAT_TREE_ID").unwrap_or_else(|_| "tree-local".into());
                    let agent_id = std::env::var("AICHAT_AGENT_NAME").unwrap_or_else(|_| "orchestrator".into());
                    let journal_dir = crate::safety::RollbackJournal::resolve_journal_dir(&config.read().safety.escalation_dir);
                    if let Ok(journal) = crate::safety::RollbackJournal::open(&journal_dir, &tree_id, &agent_id) {
                        let outcome = journal.replay_last().await?;
                        return Ok(json!({"error": {"type": "escalation_reverted", "details": outcome.details}}));
                    }
                    return Ok(json!({"error": {"type": "escalation_reverted", "message": "No journal found to replay"}}));
                }
            }
        } else {
            return Ok(denied);
        }
    }

    // Backlog #6c: `%assess-risk%` LLM evaluator overlay.
    // The evaluator can only TIGHTEN restrictions, never relax them ("the LLM is not a Pardoner").
    // Mandatory for all non-safe actions.
    if let Some(denied) = risk_evaluator_denied_result(config, call, proven_reversible_applied, risk_cache, progress).await {
        let is_child = current_agent_depth() > 0 || crate::escalation::ParentConnInfo::from_env().is_some();
        if is_child {
            // Sub-agents fail closed on risk denial; cannot elevate ceiling or bypass risk over mTLS
            return Ok(denied);
        }
        let (static_tier, _) = tool_tier_and_reversibility(config, &call.name);
        let blast_radius = match static_tier {
            crate::function::StaticTier::Tier(t) => t,
            crate::function::StaticTier::Unclassified => crate::function::BlastRadius::Catastrophic,
        };
        if *IS_STDOUT_TERMINAL {
            let ceiling = current_authority_ceiling(config);
            let decision = prompt_human_verdict(&call.name, &call.arguments, blast_radius, "risk_blocked", ceiling, progress)?;
            match decision {
                crate::safety::VerdictDecision::Continue => {}
                crate::safety::VerdictDecision::Halt => {
                    return Ok(json!({"error": {"type": "escalation_halted", "message": "Action halted by human operator"}}));
                }
                crate::safety::VerdictDecision::Revert => {
                    let tree_id = std::env::var("AICHAT_TREE_ID").unwrap_or_else(|_| "tree-local".into());
                    let agent_id = std::env::var("AICHAT_AGENT_NAME").unwrap_or_else(|_| "orchestrator".into());
                    let journal_dir = crate::safety::RollbackJournal::resolve_journal_dir(&config.read().safety.escalation_dir);
                    if let Ok(journal) = crate::safety::RollbackJournal::open(&journal_dir, &tree_id, &agent_id) {
                        let outcome = journal.replay_last().await?;
                        return Ok(json!({"error": {"type": "escalation_reverted", "details": outcome.details}}));
                    }
                    return Ok(json!({"error": {"type": "escalation_reverted", "message": "No journal found to replay"}}));
                }
            }
        } else {
            return Ok(denied);
        }
    }

    // Record pre-mutation entry in durable journal (FR-6d.6)
    if !proven_reversible_applied {
        record_pre_mutation_journal_entry(config, call, progress);
    }

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
        if call_targets_agent(config, &call.name) {
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
    if std::env::var("AICHAT_WSLINKS").map(|v| v == "true" || v == "1").unwrap_or(false) {
        cmd.arg("--wslinks");
        cmd.env("AICHAT_WSLINKS", "true");
    }
    cmd.arg(&task_message);

    // Pass depth to child
    cmd.env("AICHAT_AGENT_DEPTH", (current_depth + 1).to_string());
    cmd.env("AICHAT_AGENT_NAME", &agent_name);
    if let Ok(start_ms) = std::env::var("AICHAT_START_TIME_MS") {
        cmd.env("AICHAT_START_TIME_MS", start_ms);
    }

    // Backlog #6d (FR-6d.18): hierarchical upfront permission provisioning.
    let parent_is_readonly = under_readonly_mask();
    let parent_ceiling = current_authority_ceiling(config);
    let (provisioned_mask, provisioned_ceiling) = match crate::function::DelegatedPermissions::resolve_for_call(
        &call.arguments,
        parent_is_readonly,
        parent_ceiling,
    ) {
        Ok(perms) => perms,
        Err(err) => {
            return Ok((
                json!({
                    "error": {
                        "type": "delegation_permission_exceeded",
                        "message": format!("Permission provisioning failed: {err}")
                    }
                }),
                0.0,
            ));
        }
    };

    cmd.env("AICHAT_CAPABILITY_MASK", &provisioned_mask);
    cmd.env("AICHAT_AUTHORITY_CEILING", provisioned_ceiling.tier().as_str());

    // Backlog #6d: pass escalation listener connection parameters to child
    if let Ok(Some(listener)) = get_or_init_parent_listener(config).await {
        if let Ok(envs) = listener.child_env() {
            for (k, v) in envs {
                cmd.env(k, v);
            }
        }
    }

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
        let parsed_json = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .or_else(|| {
                text.rfind("{\"status\":").and_then(|idx| {
                    serde_json::from_str::<serde_json::Value>(&text[idx..]).ok()
                })
            });
        if let Some(val) = parsed_json {
            if val.get("status").and_then(|s| s.as_str()) == Some("permission_blocked") {
                let attempted_tool = val
                    .get("attempted_tool")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");
                let reason = val
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("capability_denied");
                let suggested_ceiling = val
                    .get("required_permission")
                    .and_then(|p| p.get("ceiling"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("disruptive");
                let guidance = if reason == "authority_exceeded" {
                    format!(
                        "Sub-agent '{}' was blocked because tool '{}' requires authority ceiling '{}', \
                         which exceeds its provisioned ceiling. Pre-mutation entries were unwound. \
                         If this action is authorized and within your ceiling, re-delegate to '{}' with \
                         permissions: {{ mask: \"mutating\", ceiling: \"{}\" }} \
                         (or flat args permissions_mask=\"mutating\", permissions_ceiling=\"{}\") or execute directly.",
                        agent_name, attempted_tool, suggested_ceiling, agent_name, suggested_ceiling, suggested_ceiling
                    )
                } else {
                    format!(
                        "Sub-agent '{}' was blocked by its read-only permission mask when attempting '{}' (reason: {}). \
                         Pre-mutation entries were unwound. If this action is authorized and within your ceiling, \
                         re-delegate to '{}' with permissions: {{ mask: \"mutating\", ceiling: \"{}\" }} \
                         (or flat args permissions_mask=\"mutating\", permissions_ceiling=\"{}\").",
                        agent_name, attempted_tool, reason, agent_name, suggested_ceiling, suggested_ceiling
                    )
                };
                return Ok((
                    json!({
                        "status": "permission_blocked",
                        "agent": agent_name,
                        "details": val,
                        "guidance": guidance,
                    }),
                    sub_cost,
                ));
            }
        }
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
        Some(raw) if output.as_object().is_some_and(|o| o.len() == 1) => raw.to_string(),
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

    // Execute the target tool. No shared risk cache here (this is a derived
    // pipe-target actuation outside the turn loop) — it is still fully gated,
    // just evaluated fresh rather than cache-reused.
    let result = eval_single_tool(config, &pipe_call, None, None).await?;

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
// Circuit breaker & budget helpers (extracted from `run` for testability)
// ---------------------------------------------------------------------------

/// After this many consecutive failures of the same tool within one loop run,
/// the tool is "tripped" and further calls short-circuit to an error.
const CIRCUIT_BREAKER_THRESHOLD: usize = 3;

/// Update circuit-breaker state from a batch of tool results.
///
/// For each result: an error increments the tool's consecutive-failure count and,
/// on reaching `CIRCUIT_BREAKER_THRESHOLD`, inserts it into `tripped`; a success
/// resets (removes) the tool's counter. Mirrors the original inline logic exactly.
fn update_circuit_breaker(
    results: &[ToolResult],
    failure_counts: &mut std::collections::HashMap<String, usize>,
    tripped: &mut std::collections::HashSet<String>,
) {
    for result in results {
        let name = &result.call.name;
        let is_error = result.output.get("error").is_some();
        if is_error {
            let count = failure_counts.entry(name.clone()).or_insert(0);
            *count += 1;
            if *count >= CIRCUIT_BREAKER_THRESHOLD && tripped.insert(name.clone()) {
                warn!(
                    "Circuit breaker tripped for tool '{}' after {} consecutive failures",
                    name, count
                );
            }
        } else {
            failure_counts.remove(name);
        }
    }
}

/// Whether the accumulated cost has exceeded the configured budget.
/// A `max_cost` of `0.0` (or negative) means "no limit".
fn cost_budget_exceeded(cost: f64, max_cost: f64) -> bool {
    max_cost > 0.0 && cost > max_cost
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
            Always invoke this tool using its exact name '_plan' (with a leading underscore, do not call 'plan'). \
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
        // The planning scratchpad never touches state — safe under any mask.
        mode: Some(crate::function::ToolMode::Readonly),
        // #6b: an internal read-only scratchpad is the lowest blast radius.
        risk: Some(crate::function::BlastRadius::Safe),
        reversible: Some(true),
        reversible_via: None,
        nano: None,
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
    // Eagerly initialize persistent child client if running under parent listener (#6d)
    let _ = get_or_init_child_client().await;

    let max_turns = params.config.read().agent_loop.max_turns;
    let max_cost = params.config.read().agent_loop.max_cost;
    let model = input.role().model().clone();
    let mut current_input = input;
    let mut total_usage = TokenUsage::default();
    let mut last_text = String::new();

    // Circuit breaker: track consecutive failures per tool name.
    // After 3 consecutive failures, the tool is "tripped" and further calls
    // return an error immediately without execution.
    let mut tool_failure_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut tripped_tools: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut redelegation_counts: std::collections::HashMap<(String, String), usize> = std::collections::HashMap::new();

    // #6c: a monotonic, raise-only risk-verdict cache shared across all turns of
    // this run, so an identical action assessed once is not re-evaluated (and so a
    // future plan-time pre-pass can pre-raise an action's floor). Raise-only means
    // it can only ever make the gate stricter — never green-light.
    let risk_cache = std::sync::Arc::new(parking_lot::Mutex::new(crate::safety::RiskCache::new()));
    let agent_name = current_agent_name(params.config);
    let pid = std::process::id();

    for turn in 1..=max_turns {
        params.progress.set_turn(turn, max_turns);
        params.progress.emit(AgentLoopEvent::TurnStart { turn, max_turns });

        // On the first turn, record the initial state (matches old before_chat_completion call)
        if turn == 1 {
            params.config.write().before_chat_completion(&current_input)?;
        }

        if params.config.read().agent_loop.show_dialog {
            let no_truncate = params.config.read().agent_loop.dialog_no_truncate;
            let prompt_display = match current_input.build_messages() {
                Ok(mut msgs) => {
                    crate::client::patch_messages(&mut msgs, &model);
                    format_messages_dialog_with_turn(&msgs, no_truncate, turn)
                }
                Err(e) => format!("(failed to format messages: {e})"),
            };
            params.progress.emit(AgentLoopEvent::DialogBlock {
                agent: agent_name.clone(),
                pid,
                turn,
                max_turns,
                direction: DialogDirection::Request,
                content: prompt_display,
            });
        }

        // 1. Call the LLM (returns raw tool_calls, does not eval them)
        let call_res = call_llm_raw(&current_input, &params).await;
        let (output, tool_calls) = match call_res {
            Ok(val) => {
                if params.config.read().agent_loop.show_dialog {
                    let no_truncate = params.config.read().agent_loop.dialog_no_truncate;
                    let response_display = format_llm_response(&val.0, &val.1, no_truncate);
                    params.progress.emit(AgentLoopEvent::DialogBlock {
                        agent: agent_name.clone(),
                        pid,
                        turn,
                        max_turns,
                        direction: DialogDirection::Response,
                        content: response_display,
                    });
                }
                val
            }
            Err(err) => {
                if params.config.read().agent_loop.show_dialog {
                    params.progress.emit(AgentLoopEvent::DialogBlock {
                        agent: agent_name.clone(),
                        pid,
                        turn,
                        max_turns,
                        direction: DialogDirection::Response,
                        content: format!("(LLM call failed: {err})"),
                    });
                }
                return Err(err);
            }
        };

        total_usage.add(output.usage());
        last_text = output.text.clone();

        // Track cost
        if let Some(turn_cost) = model.usage_cost(output.usage()) {
            params.progress.add_cost(turn_cost);
        }

        // Cost budget check
        if cost_budget_exceeded(params.progress.cost(), max_cost) {
            params.progress.emit(AgentLoopEvent::CostExhausted {
                cost: params.progress.cost(),
                max_cost,
            });
            eprintln!(
                "Warning: Agent loop exceeded the ${:.4} cost limit (spent ${:.4}). \
                 Increase with `agent_loop.max_cost` in config.yaml or AICHAT_AGENT_LOOP_MAX_COST=N.",
                max_cost, params.progress.cost()
            );
            if let Some(client) = get_or_init_child_client().await {
                let _ = client
                    .send_result(
                        Ok(serde_json::json!({ "final_text": last_text, "cost_exhausted": true })),
                        params.progress.cost(),
                    )
                    .await;
            }
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

            if let Some(client) = get_or_init_child_client().await {
                let _ = client
                    .send_result(
                        Ok(serde_json::json!({ "final_text": output.text })),
                        params.progress.cost(),
                    )
                    .await;
            } else if let Some(parent_info) = crate::escalation::ParentConnInfo::from_env() {
                let depth = std::env::var("AICHAT_AGENT_DEPTH").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                let agent_id = std::env::var("AICHAT_AGENT_NAME").unwrap_or_else(|_| format!("agent-d{depth}"));
                let _ = crate::escalation::notify_parent_result(
                    &parent_info,
                    &agent_id,
                    depth,
                    Ok(serde_json::json!({ "final_text": output.text })),
                    params.progress.cost(),
                )
                .await;
            }

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
            real_calls.into_iter().partition(|c| {
                if tripped_tools.contains(&c.name) {
                    return true;
                }
                if call_targets_agent(params.config, &c.name) {
                    let task_key = c.arguments.get("task").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if redelegation_counts.get(&(c.name.clone(), task_key)).copied().unwrap_or(0) >= 2 {
                        return true;
                    }
                }
                false
            });

        // Return immediate errors for tripped tools
        for call in &tripped_calls {
            let msg = if call_targets_agent(params.config, &call.name) {
                format!(
                    "Sub-agent '{}' re-delegation circuit breaker tripped after repeated permission blocks on the same task. Escalate to the human or change your delegation strategy.",
                    call.name
                )
            } else {
                format!(
                    "Tool '{}' has been disabled after {} consecutive failures. \
                     Use a different tool or approach.",
                    call.name, CIRCUIT_BREAKER_THRESHOLD
                )
            };
            tool_results.push(ToolResult::new(
                call.clone(),
                json!({
                    "error": {
                        "type": "circuit_breaker",
                        "message": msg
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
                Some(risk_cache.clone()),
            )
            .await?;

            // Update circuit breaker state based on results
            update_circuit_breaker(&real_results, &mut tool_failure_counts, &mut tripped_tools);

            for res in &real_results {
                if res.output.get("status").and_then(|s| s.as_str()) == Some("permission_blocked") {
                    let task_key = res.call.arguments.get("task").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let count = redelegation_counts.entry((res.call.name.clone(), task_key)).or_insert(0);
                    *count += 1;
                }
            }

            tool_results.extend(real_results);
        }

        // Backlog #6d (FR-6d.19 & FR-6d.24): Unified process sandbox boundary for sub-agents.
        // If running under a readonly capability mask (capability_denied) or as a child process
        // with an authority ceiling violation (authority_exceeded), halt actuation immediately,
        // unwind durable journal entries, and exit cleanly with status: "permission_blocked".
        let blocked_denial = tool_results.iter().find(|r| {
            if let Some(err_type) = r.output.get("error").and_then(|e| e.get("type")).and_then(|t| t.as_str()) {
                if err_type == "capability_denied" && under_readonly_mask() {
                    return true;
                }
                let is_child = current_agent_depth() > 0 || crate::escalation::ParentConnInfo::from_env().is_some();
                if (err_type == "authority_exceeded" || err_type == "risk_blocked") && is_child {
                    return true;
                }
            }
            false
        });

        if let Some(denied_res) = blocked_denial {
            let tree_id = std::env::var("AICHAT_TREE_ID").unwrap_or_else(|_| "tree-local".into());
            let agent_id = std::env::var("AICHAT_AGENT_NAME").unwrap_or_else(|_| "agent".into());
            let journal_dir = crate::safety::RollbackJournal::resolve_journal_dir(&params.config.read().safety.escalation_dir);
            let unwound = if let Ok(journal) = crate::safety::RollbackJournal::open(&journal_dir, &tree_id, &agent_id) {
                journal.replay_last().await.is_ok()
            } else {
                false
            };

            params.progress.emit(AgentLoopEvent::CapabilityBlocked {
                name: denied_res.call.name.clone(),
                unwound,
            });

            let tool_decl = find_tool_declaration(params.config, &denied_res.call.name);
            let (static_tier, _) = tool_tier_and_reversibility(params.config, &denied_res.call.name);
            let static_tier_str = match static_tier {
                crate::function::StaticTier::Tier(t) => t.as_str(),
                crate::function::StaticTier::Unclassified => "catastrophic",
            };
            let required_ceiling = tool_decl
                .as_ref()
                .and_then(|d| d.risk)
                .map(|r| r.as_str())
                .unwrap_or(static_tier_str);

            let reason = denied_res
                .output
                .get("error")
                .and_then(|e| e.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("capability_denied");

            let payload = json!({
                "status": "permission_blocked",
                "attempted_tool": denied_res.call.name,
                "arguments": denied_res.call.arguments,
                "required_permission": {
                    "mask": "mutating",
                    "ceiling": required_ceiling,
                },
                "reason": reason,
                "rollback_executed": unwound,
                "triage_summary": output.text,
            });

                let payload_str = payload.to_string();

                if let Some(client) = get_or_init_child_client().await {
                    let _ = client
                        .send_result(
                            Ok(payload.clone()),
                            params.progress.cost(),
                        )
                        .await;
                } else if let Some(parent_info) = crate::escalation::ParentConnInfo::from_env() {
                    let depth = std::env::var("AICHAT_AGENT_DEPTH").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                    let _ = crate::escalation::notify_parent_result(
                        &parent_info,
                        &agent_id,
                        depth,
                        Ok(payload.clone()),
                        params.progress.cost(),
                    )
                    .await;
                }

                println!("{payload_str}");

                params
                    .config
                    .write()
                    .after_chat_completion(&current_input, &payload_str, &tool_results)?;

                return Ok(AgentLoopOutput {
                    usage: total_usage,
                    final_text: payload_str,
                });
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

    if let Some(client) = get_or_init_child_client().await {
        let _ = client
            .send_result(
                Ok(serde_json::json!({ "final_text": last_text, "budget_exhausted": true })),
                params.progress.cost(),
            )
            .await;
    }

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

    let mut retries = 0;
    const MAX_EMPTY_RETRIES: usize = 3;

    loop {
        if params.abort_signal.aborted() {
            bail!("Aborted.");
        }

        let res = if !input.stream() || extract_code {
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
        };

        match res {
            Ok((output, tool_calls)) => {
                if output.text.trim().is_empty() && tool_calls.is_empty() {
                    if retries < MAX_EMPTY_RETRIES && !params.abort_signal.aborted() {
                        retries += 1;
                        let (base_ms, jitter_range_ms) = match retries {
                            1 => (1000, 200),
                            2 => (2500, 300),
                            _ => (5000, 500),
                        };
                        let random_u32 = u32::from_le_bytes(
                            uuid::Uuid::new_v4().as_bytes()[0..4]
                                .try_into()
                                .unwrap(),
                        );
                        let jitter = (random_u32 % (2 * jitter_range_ms + 1)) as i64 - jitter_range_ms as i64;
                        let delay_ms = (base_ms as i64 + jitter).max(100) as u64;

                        log::debug!(
                            "LLM returned empty response (attempt {}/{}), retrying in {}ms...",
                            retries,
                            MAX_EMPTY_RETRIES,
                            delay_ms
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                    bail!("LLM returned an empty response with no text and no tool calls");
                }
                return Ok((output, tool_calls));
            }
            Err(err) => {
                let err_str = err.to_string();
                if (err_str.contains("MALFORMED_FUNCTION_CALL")
                    || err_str.contains("ResourceExhausted")
                    || err_str.contains("rate limit")
                    || err_str.contains("429")
                    || err_str.contains("503")
                    || err_str.contains("connection reset"))
                    && retries < MAX_EMPTY_RETRIES
                    && !params.abort_signal.aborted()
                {
                    retries += 1;
                    let (base_ms, jitter_range_ms) = match retries {
                        1 => (1000, 200),
                        2 => (2500, 300),
                        _ => (5000, 500),
                    };
                    let random_u32 = u32::from_le_bytes(
                        uuid::Uuid::new_v4().as_bytes()[0..4]
                            .try_into()
                            .unwrap(),
                    );
                    let jitter = (random_u32 % (2 * jitter_range_ms + 1)) as i64 - jitter_range_ms as i64;
                    let delay_ms = (base_ms as i64 + jitter).max(100) as u64;

                    log::warn!(
                        "LLM call transient error: {err_str} (attempt {}/{}), retrying in {}ms...",
                        retries,
                        MAX_EMPTY_RETRIES,
                        delay_ms
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
                return Err(err);
            }
        }
    }
}


// ---------------------------------------------------------------------------
// Observability: rendering, OSC titles, status file, notifications
// ---------------------------------------------------------------------------

use crate::config::AgentLoopConfig;
use serde::Serialize;
use std::io::Write;

/// Helper to identify the executing agent for trace / dialog observability.
pub fn current_agent_name(config: &GlobalConfig) -> String {
    config
        .read()
        .agent
        .as_ref()
        .map(|a| a.name().to_string())
        .or_else(|| config.read().role.as_ref().map(|r| r.name().to_string()))
        .or_else(|| {
            std::env::var("AICHAT_AGENT_NAME")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            std::env::var("AICHAT_INVOKING_AGENT")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|inv| format!("nano-{inv}"))
        })
        .unwrap_or_else(|| "aichat".to_string())
}

/// Helper to get current agent nesting depth (0 for root/orchestrator).
pub fn current_agent_depth() -> usize {
    std::env::var("AICHAT_AGENT_DEPTH")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
}

pub const ERROR_COLOR: nu_ansi_term::Color = nu_ansi_term::Color::Rgb(224, 108, 117);
pub const ESCALATION_COLOR: nu_ansi_term::Color = nu_ansi_term::Color::Rgb(209, 154, 102);

pub const AGENT_PALETTE: &[(&str, nu_ansi_term::Color)] = &[
    ("cyan", nu_ansi_term::Color::Cyan),
    ("green", nu_ansi_term::Color::Green),
    ("yellow", nu_ansi_term::Color::Yellow),
    ("purple", nu_ansi_term::Color::Purple),
    ("light_blue", nu_ansi_term::Color::LightBlue),
    ("light_cyan", nu_ansi_term::Color::LightCyan),
    ("light_green", nu_ansi_term::Color::LightGreen),
    ("light_yellow", nu_ansi_term::Color::LightYellow),
    ("light_magenta", nu_ansi_term::Color::LightMagenta),
    ("blue", nu_ansi_term::Color::Blue),
    ("magenta", nu_ansi_term::Color::Magenta),
];

static AGENT_LABEL_COLORS: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashMap<String, usize>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// Helper to get or generate an ephemeral random color for an agent label, honoring inherited colors.
pub fn agent_color(name: &str) -> nu_ansi_term::Color {
    if name == "%assess-risk%" || name == "assess-risk" {
        return nu_ansi_term::Color::Red;
    }
    if name == "%functions%" {
        return nu_ansi_term::Color::LightCyan;
    }

    if let Ok(color_str) = std::env::var("AICHAT_AGENT_COLOR") {
        if let Some(c) = color_from_name(&color_str) {
            return c;
        }
    }

    let base_name = name.to_lowercase();
    let clean_name = base_name.strip_prefix("nano-").unwrap_or(&base_name);

    let mut map = AGENT_LABEL_COLORS.lock();
    if let Some(&idx) = map.get(clean_name) {
        return AGENT_PALETTE[idx].1;
    }

    let used_indices: std::collections::HashSet<usize> = map.values().copied().collect();
    let seed = (std::process::id() as usize)
        .wrapping_mul(0x9E3779B9)
        ^ (clean_name.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize)));
    let offset = seed % AGENT_PALETTE.len();
    let mut chosen = offset;
    for i in 0..AGENT_PALETTE.len() {
        let candidate = (offset + i) % AGENT_PALETTE.len();
        if !used_indices.contains(&candidate) {
            chosen = candidate;
            break;
        }
    }
    map.insert(clean_name.to_string(), chosen);
    AGENT_PALETTE[chosen].1
}

/// Helper to get the string name of an agent label's color for inheritance across processes.
pub fn current_agent_color_name(name: &str) -> &'static str {
    let color = agent_color(name);
    color_to_name(color)
}

pub fn color_to_name(color: nu_ansi_term::Color) -> &'static str {
    for &(name, c) in AGENT_PALETTE {
        if c == color {
            return name;
        }
    }
    "cyan"
}

pub fn color_from_name(name: &str) -> Option<nu_ansi_term::Color> {
    for &(n, c) in AGENT_PALETTE {
        if n == name {
            return Some(c);
        }
    }
    None
}

/// Truncate long lines horizontally to avoid terminal blowout.
fn truncate_line_width(line: &str, max_len: usize) -> String {
    if line.len() > max_len {
        let keep = max_len / 2;
        format!(
            "{}... (truncated {} bytes) ...{}",
            &line[..keep],
            line.len() - max_len,
            &line[line.len() - keep..]
        )
    } else {
        line.to_string()
    }
}

/// Truncates payload text for dialog observability trace to at most top N lines and bottom N lines.
/// Preserves full content if lines <= top + bottom or if no_truncate is true.
pub fn truncate_payload_dialog(text: &str, top: usize, bottom: usize, no_truncate: bool) -> String {
    if no_truncate {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > top + bottom {
        let omitted = lines.len() - top - bottom;
        let top_lines = lines[..top]
            .iter()
            .map(|l| truncate_line_width(l, 2000))
            .collect::<Vec<_>>()
            .join("\n");
        let bottom_lines = lines[lines.len() - bottom..]
            .iter()
            .map(|l| truncate_line_width(l, 2000))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{top_lines}\n... (payload truncated: {omitted} lines omitted) ...\n{bottom_lines}")
    } else if lines.iter().any(|l| l.len() > 2000) {
        lines
            .iter()
            .map(|l| truncate_line_width(l, 2000))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text.to_string()
    }
}

/// Format messages submitted to LLM for dialog observability trace.
/// The instruction portion (system prompt) is shown in full on turn 1, or folded to a summary on turn > 1 if unchanged and > 3 lines.
/// Payloads (user messages, assistant messages, tool results) are truncated to at most top 20 and last 20 lines unless no_truncate is true.
pub fn format_messages_dialog(messages: &[crate::client::Message], no_truncate: bool) -> String {
    format_messages_dialog_with_turn(messages, no_truncate, 1)
}

/// Format LLM response text, dimming Markdown blockquote lines (`> ...`) in DarkGray.
pub fn format_response_text_with_blockquotes(text: &str) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with('>') {
            lines.push(nu_ansi_term::Color::DarkGray.paint(line).to_string());
        } else {
            lines.push(line.to_string());
        }
    }
    lines.join("\n")
}

/// Format messages submitted to LLM for dialog observability trace with turn awareness and semantic styling.
pub fn format_messages_dialog_with_turn(
    messages: &[crate::client::Message],
    no_truncate: bool,
    turn: usize,
) -> String {
    use crate::client::{MessageContent, MessageContentPart, MessageRole};
    let mut out = String::new();
    let num_messages = messages.len();

    for (i, msg) in messages.iter().enumerate() {
        let is_last = i == num_messages - 1;
        let is_history = num_messages > 2 && !is_last && i > 0;

        let (role_header, content_str) = match msg.role {
            MessageRole::System => {
                let raw_text = match &msg.content {
                    MessageContent::Text(t) => t.clone(),
                    MessageContent::Array(parts) => parts
                        .iter()
                        .map(|p| match p {
                            MessageContentPart::Text { text } => text.as_str(),
                            MessageContentPart::ImageUrl { .. } => "[image]",
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                };
                let line_count = raw_text.lines().count();
                if turn > 1 && line_count > 3 {
                    (
                        nu_ansi_term::Color::DarkGray
                            .paint(format!("[system: {line_count} lines instructions unchanged]"))
                            .to_string(),
                        String::new(),
                    )
                } else {
                    let badge = nu_ansi_term::Color::LightCyan.bold().paint("[system]").to_string();
                    let dimmed_corpus = nu_ansi_term::Color::DarkGray.paint(&raw_text).to_string();
                    (badge, dimmed_corpus)
                }
            }
            MessageRole::User => {
                let text = match &msg.content {
                    MessageContent::Text(t) => truncate_payload_dialog(t, 20, 20, no_truncate),
                    MessageContent::Array(parts) => {
                        let combined = parts
                            .iter()
                            .map(|p| match p {
                                MessageContentPart::Text { text } => text.as_str(),
                                MessageContentPart::ImageUrl { .. } => "[image]",
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        truncate_payload_dialog(&combined, 20, 20, no_truncate)
                    }
                    _ => String::new(),
                };
                if is_history {
                    let badge = format!(
                        "[{}: {}]",
                        ESCALATION_COLOR.bold().paint("history"),
                        nu_ansi_term::Color::Cyan.bold().paint("user")
                    );
                    (badge, nu_ansi_term::Color::DarkGray.paint(&text).to_string())
                } else if turn > 1 {
                    let badge = format!(
                        "{} {}",
                        nu_ansi_term::Color::Yellow.bold().paint("⚡"),
                        nu_ansi_term::Color::Cyan.bold().paint("[new: user]")
                    );
                    (badge, text)
                } else {
                    (nu_ansi_term::Color::Cyan.bold().paint("[user]").to_string(), text)
                }
            }
            MessageRole::Assistant => {
                let text = match &msg.content {
                    MessageContent::Text(t) => truncate_payload_dialog(t, 20, 20, no_truncate),
                    MessageContent::Array(parts) => {
                        let combined = parts
                            .iter()
                            .map(|p| match p {
                                MessageContentPart::Text { text } => text.as_str(),
                                MessageContentPart::ImageUrl { .. } => "[image]",
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        truncate_payload_dialog(&combined, 20, 20, no_truncate)
                    }
                    _ => String::new(),
                };
                if is_history {
                    let badge = format!(
                        "[{}: {}]",
                        ESCALATION_COLOR.bold().paint("history"),
                        nu_ansi_term::Color::Yellow.bold().paint("assistant")
                    );
                    (badge, nu_ansi_term::Color::DarkGray.paint(&text).to_string())
                } else if turn > 1 {
                    let badge = format!(
                        "{} {}",
                        nu_ansi_term::Color::Yellow.bold().paint("⚡"),
                        nu_ansi_term::Color::Yellow.bold().paint("[new: assistant]")
                    );
                    (badge, text)
                } else {
                    (nu_ansi_term::Color::Yellow.bold().paint("[assistant]").to_string(), text)
                }
            }
            MessageRole::Tool => {
                let text = match &msg.content {
                    MessageContent::Text(t) => truncate_payload_dialog(t, 20, 20, no_truncate),
                    _ => String::new(),
                };
                if is_history {
                    let badge = format!(
                        "[{}: {}]",
                        ESCALATION_COLOR.bold().paint("history"),
                        nu_ansi_term::Color::Magenta.bold().paint("tool")
                    );
                    (badge, nu_ansi_term::Color::DarkGray.paint(&text).to_string())
                } else {
                    let badge = format!(
                        "{} {}",
                        nu_ansi_term::Color::Yellow.bold().paint("⚡"),
                        nu_ansi_term::Color::Magenta.bold().paint("[new: tool]")
                    );
                    (badge, text)
                }
            }
        };

        let (role_header, final_content) = if let MessageContent::ToolCalls(tc) = &msg.content {
            let mut parts = Vec::new();
            if !tc.text.is_empty() {
                let text_trunc = truncate_payload_dialog(&tc.text, 20, 20, no_truncate);
                if is_history {
                    parts.push(nu_ansi_term::Color::DarkGray.paint(&text_trunc).to_string());
                } else {
                    parts.push(text_trunc);
                }
            }
            for res in &tc.tool_results {
                let output_str = if let Some(s) = res.output.as_str() {
                    s.to_string()
                } else if let Some(s) = res.output.get("output").and_then(|v| v.as_str()) {
                    s.to_string()
                } else if let Ok(s) = serde_json::to_string_pretty(&res.output) {
                    s
                } else {
                    res.output.to_string()
                };
                let truncated_output = truncate_payload_dialog(&output_str, 20, 20, no_truncate);
                if is_history {
                    parts.push(format!(
                        "{} {}",
                        nu_ansi_term::Color::DarkGray.paint(format!("tool_result: {} ->", res.call.name)),
                        nu_ansi_term::Color::DarkGray.paint(&truncated_output)
                    ));
                } else {
                    let badge = format!(
                        "{} {}",
                        nu_ansi_term::Color::Yellow.bold().paint("⚡"),
                        nu_ansi_term::Color::Magenta.bold().paint(format!("[new: tool_result: {}] ->", res.call.name))
                    );
                    parts.push(format!("{badge} {truncated_output}"));
                }
            }

            let effective_header = if !tc.tool_results.is_empty() && tc.text.is_empty() {
                if is_history {
                    format!(
                        "[{}: {}]",
                        ESCALATION_COLOR.bold().paint("history"),
                        nu_ansi_term::Color::Magenta.bold().paint("tool_results")
                    )
                } else {
                    format!(
                        "{} {}",
                        nu_ansi_term::Color::Yellow.bold().paint("⚡"),
                        nu_ansi_term::Color::Magenta.bold().paint("[new: tool_results]")
                    )
                }
            } else {
                role_header
            };

            (effective_header, parts.join("\n"))
        } else {
            (role_header, content_str)
        };

        if i > 0 {
            out.push_str(&nu_ansi_term::Color::DarkGray.paint("\n───\n").to_string());
        }
        if final_content.is_empty() {
            out.push_str(&role_header);
        } else {
            out.push_str(&format!("{role_header}\n{final_content}"));
        }
    }
    out
}

/// Format the LLM response (text and/or tool calls) for dialog observability trace.
pub fn format_llm_response(
    output: &ChatCompletionsOutput,
    tool_calls: &[crate::client::ToolCall],
    no_truncate: bool,
) -> String {
    let mut parts = Vec::new();
    if !output.text.trim().is_empty() {
        let text = truncate_payload_dialog(output.text.trim(), 20, 20, no_truncate);
        parts.push(format_response_text_with_blockquotes(&text));
    }
    if !tool_calls.is_empty() {
        let calls_val: Vec<_> = tool_calls
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "arguments": c.arguments,
                })
            })
            .collect();
        let tc_badge = nu_ansi_term::Color::LightBlue.bold().paint("tool_calls:").to_string();
        if let Ok(calls_str) = serde_json::to_string_pretty(&calls_val) {
            parts.push(format!("{tc_badge}\n{}", truncate_payload_dialog(&calls_str, 20, 20, no_truncate)));
        } else {
            parts.push(format!("{tc_badge} {:?}", tool_calls));
        }
    }
    if parts.is_empty() {
        nu_ansi_term::Style::new().dimmed().paint("(empty response)").to_string()
    } else {
        parts.join("\n\n")
    }
}

/// Strip ANSI escape sequences from a string to get clean visible text.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(&'[') = chars.peek() {
                chars.next(); // consume '['
                for c2 in chars.by_ref() {
                    if ('@'..='~').contains(&c2) {
                        break;
                    }
                }
                continue;
            } else if let Some(&']') = chars.peek() {
                chars.next(); // consume ']'
                for c2 in chars.by_ref() {
                    if c2 == '\x07' || c2 == '\x1b' {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Compute visible terminal column width of a string, ignoring ANSI escape sequences.
pub fn visible_width(s: &str) -> usize {
    let clean = strip_ansi(s);
    UnicodeWidthStr::width(clean.as_str())
}

/// Query active terminal width, supporting environment overrides (`AICHAT_TERMINAL_WIDTH`, `COLUMNS`)
/// and crossterm detection, with safe default.
pub fn get_terminal_width() -> usize {
    if let Ok(val) = std::env::var("AICHAT_TERMINAL_WIDTH") {
        if let Ok(w) = val.parse::<usize>() {
            if w >= 20 {
                return w;
            }
        }
    }
    if let Ok(val) = std::env::var("COLUMNS") {
        if let Ok(w) = val.parse::<usize>() {
            if w >= 20 {
                return w;
            }
        }
    }
    if let Ok((cols, _)) = crossterm::terminal::size() {
        if cols >= 20 {
            return cols as usize;
        }
    }
    80
}

/// Generate vertical guide rails for ancestor agents up to `depth`.
pub fn ancestor_rails(depth: usize) -> String {
    let mut rails = String::new();
    for d in 0..depth {
        let rail = match d {
            0 => "│     ",
            1 => "║     ",
            _ => "╏     ",
        };
        rails.push_str(rail);
    }
    rails
}

/// Detect the ideal continuation indent for a wrapped line.
pub fn detect_continuation_indent(line: &str) -> &'static str {
    let stripped = strip_ansi(line);
    let trimmed = stripped.trim_start();
    let leading_spaces = stripped.len() - trimmed.len();

    if leading_spaces >= 8 {
        "          "
    } else if leading_spaces >= 6 {
        "        "
    } else if leading_spaces >= 4 {
        "      "
    } else if leading_spaces >= 2
        || trimmed.starts_with("* ")
        || trimmed.starts_with("- ")
        || stripped.contains('⚡')
        || stripped.contains("[new:")
        || stripped.contains("[history:")
        || stripped.contains("tool_result:")
    {
        "    "
    } else {
        ""
    }
}

/// Soft-wrap a line containing ANSI styling into chunks of at most `max_width` visible columns.
/// Preserves active ANSI styling across line wraps and applies `continuation_indent` to wrapped chunks.
pub fn wrap_ansi_line(line: &str, max_width: usize, continuation_indent: &str) -> Vec<String> {
    if visible_width(line) <= max_width {
        return vec![line.to_string()];
    }

    #[derive(Debug)]
    struct Token<'a> {
        text: &'a str,
        is_escape: bool,
        is_whitespace: bool,
        width: usize,
    }

    let mut tokens = Vec::new();
    let mut chars = line.char_indices().peekable();

    while let Some((start, c)) = chars.next() {
        if c == '\x1b' {
            if let Some(&(_, '[')) = chars.peek() {
                chars.next();
                let mut end = line.len();
                while let Some(&(i, c2)) = chars.peek() {
                    chars.next();
                    if ('@'..='~').contains(&c2) {
                        end = i + c2.len_utf8();
                        break;
                    }
                }
                tokens.push(Token {
                    text: &line[start..end],
                    is_escape: true,
                    is_whitespace: false,
                    width: 0,
                });
                continue;
            }
        }
        if c.is_whitespace() {
            let mut end = start + c.len_utf8();
            while let Some(&(i, next_c)) = chars.peek() {
                if next_c.is_whitespace() && next_c != '\x1b' {
                    chars.next();
                    end = i + next_c.len_utf8();
                } else {
                    break;
                }
            }
            let text = &line[start..end];
            let width = text.chars().map(|ch| if ch == '\t' { 4 } else { 1 }).sum();
            tokens.push(Token {
                text,
                is_escape: false,
                is_whitespace: true,
                width,
            });
        } else {
            let mut end = start + c.len_utf8();
            while let Some(&(i, next_c)) = chars.peek() {
                if next_c.is_whitespace() || next_c == '\x1b' {
                    break;
                }
                chars.next();
                end = i + next_c.len_utf8();
            }
            let text = &line[start..end];
            let width = UnicodeWidthStr::width(text);
            tokens.push(Token {
                text,
                is_escape: false,
                is_whitespace: false,
                width,
            });
        }
    }

    let continuation_indent_width = visible_width(continuation_indent);
    let mut result = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0usize;
    let mut is_first_line = true;
    let mut active_style: Option<String> = None;
    let target_width = max_width.max(10);

    for token in tokens {
        if token.is_escape {
            if token.text == "\x1b[0m" {
                active_style = None;
            } else {
                active_style = Some(token.text.to_string());
            }
            current_line.push_str(token.text);
            continue;
        }

        if token.is_whitespace {
            if current_width == 0 {
                if is_first_line {
                    current_line.push_str(token.text);
                    current_width += token.width;
                }
            } else if current_width + token.width <= target_width {
                current_line.push_str(token.text);
                current_width += token.width;
            } else {
                let trimmed = current_line.trim_end_matches(' ');
                let mut end_line = trimmed.to_string();
                if active_style.is_some() {
                    end_line.push_str("\x1b[0m");
                }
                result.push(end_line);
                current_line = continuation_indent.to_string();
                current_width = continuation_indent_width;
                if let Some(style) = &active_style {
                    current_line.push_str(style);
                }
                is_first_line = false;
            }
            continue;
        }

        let word_width = token.width;
        if current_width > continuation_indent_width && current_width + word_width > target_width {
            let trimmed = current_line.trim_end_matches(' ');
            let mut end_line = trimmed.to_string();
            if active_style.is_some() {
                end_line.push_str("\x1b[0m");
            }
            result.push(end_line);
            current_line = continuation_indent.to_string();
            current_width = continuation_indent_width;
            if let Some(style) = &active_style {
                current_line.push_str(style);
            }
            is_first_line = false;
        }

        if word_width > target_width.saturating_sub(current_width) {
            for c in token.text.chars() {
                let cw = UnicodeWidthChar::width(c).unwrap_or(1);
                if current_width + cw > target_width && current_width > continuation_indent_width {
                    if active_style.is_some() {
                        current_line.push_str("\x1b[0m");
                    }
                    result.push(current_line);
                    current_line = continuation_indent.to_string();
                    current_width = continuation_indent_width;
                    if let Some(style) = &active_style {
                        current_line.push_str(style);
                    }
                }
                current_line.push(c);
                current_width += cw;
            }
        } else {
            current_line.push_str(token.text);
            current_width += word_width;
        }
    }

    if !current_line.is_empty() {
        if active_style.is_some() {
            current_line.push_str("\x1b[0m");
        }
        result.push(current_line);
    }

    if result.is_empty() {
        result.push(String::new());
    }

    result
}

const ADJECTIVES: &[&str] = &[
    "Arch", "Balmy", "Barky", "Barmy", "Batty", "Bogus", "Bonk", "Brass",
    "Breezy", "Brief", "Brisk", "Buff", "Bumpy", "Cagey", "Camp", "Cheeky",
    "Chilly", "Chirpy", "Chokey", "Chuffed", "Clever", "Cocky", "Corking", "Cozy",
    "Crafty", "Cranky", "Crisp", "Crusty", "Curt", "Daft", "Dandy", "Dapper",
    "Deft", "Dim", "Dinky", "Dodgy", "Dogged", "Dopey", "Dotty", "Dour",
    "Dread", "Droll", "Dry", "Dunce", "Eager", "Edgy", "Faffy", "Faint",
    "Feisty", "Fickle", "Fierce", "Fishy", "Flash", "Flashy", "Fluky", "Footsy",
    "Foppish", "Foxy", "Frank", "Fretful", "Frosty", "Frumpy", "Fussy", "Gauzy",
    "Giddy", "Glib", "Glum", "Groovy", "Grubby", "Grumpy", "Gutsy", "Haughty",
    "Heady", "Hoary", "Iffy", "Jaunty", "Jellied", "Jittery", "Jolly", "Jovial",
    "Jumpy", "Keen", "Kinky", "Knobby", "Knotty", "Kooky", "Larky", "Leery",
    "Loopy", "Lucky", "Mad", "Manky", "Miffed", "Mingy", "Moody", "Mopey",
    "Mucky", "Muddy", "Murky", "Naff", "Narky", "Natty", "Naughty", "Nifty",
    "Nimble", "Nippy", "Nobby", "Nutty", "Odd", "Peaky", "Peckish", "Peppery",
    "Perky", "Pesky", "Picky", "Pithy", "Plucky", "Plum", "Posh", "Prig",
    "Pukka", "Quaint", "Quick", "Quirky", "Racy", "Rank", "Rash", "Risky",
    "Ritzy", "Roguish", "Rowdy", "Rummy", "Rusty", "Saucy", "Scabby", "Scrappy",
    "Sharp", "Shifty", "Shrewd", "Slinky", "Smart", "Smug", "Snappy", "Snide",
    "Snooty", "Sparky", "Spry", "Squiffy", "Stiff", "Stuffy", "Sulky", "Swanky",
    "Tart", "Tetchy", "Tipsy", "Tricky", "Trim", "Tubby", "Wacky", "Wary",
    "Waspish", "Wee", "Wily", "Witty", "Wonky", "Wry", "Zany", "Zippy",
];

const NOUNS: &[&str] = &[
    "Adder", "Aunt", "Badger", "Bandit", "Baron", "Beadle", "Beetle", "Bishop",
    "Bloke", "Bobby", "Boffin", "Boots", "Bounder", "Buffer", "Buffoon", "Bulldog",
    "Bumble", "Burgess", "Butler", "Buzzard", "Cad", "Cadet", "Captain", "Carrot",
    "Caster", "Chaff", "Chap", "Chaser", "Chimp", "Clerk", "Cleric", "Cloak",
    "Clod", "Clown", "Cobbler", "Codger", "Colonel", "Cornet", "Cousin", "Cricket",
    "Crony", "Cuckoo", "Curate", "Dandy", "Dean", "Dipper", "Doctor", "Dodger",
    "Don", "Dotard", "Drake", "Droog", "Duck", "Duchess", "Duffer", "Duke",
    "Earl", "Falcon", "Fellow", "Ferret", "Finch", "Flunkey", "Footman", "Fop",
    "Fox", "Friar", "Frog", "Gaffer", "Galahad", "Gander", "Geek", "Gent",
    "Geordie", "Ghillie", "Goose", "Goon", "Guide", "Gunner", "Guvnor", "Harrier",
    "Hawk", "Hedger", "Heron", "Hobnob", "Hussar", "Jackdaw", "Janitor", "Jeeves",
    "Jester", "Judge", "Juror", "Kestrel", "Knight", "Lackey", "Laird", "Lancer",
    "Lapwing", "Legate", "Llama", "Lobster", "Lord", "Lounger", "Mage", "Magpie",
    "Major", "Mallard", "Marshal", "Mayor", "Minstrel", "Mole", "Monk", "Muppet",
    "Nabob", "Newt", "Ninny", "Noddy", "Novice", "Otter", "Owl", "Page",
    "Paladin", "Palmer", "Panther", "Parson", "Pauper", "Peacock", "Peer", "Pigeon",
    "Pikeman", "Pilgrim", "Plover", "Plum", "Poacher", "Pomp", "Poodle", "Postman",
    "Prat", "Prior", "Proctor", "Pug", "Pundit", "Ranger", "Rascal", "Raven",
    "Rector", "Regent", "Rogue", "Rook", "Rover", "Rowdy", "Sailor", "Scout",
    "Scribe", "Shrew", "Skipper", "Snipe", "Spaniel", "Sparrow", "Spider", "Squire",
    "Stag", "Steward", "Swain", "Tar", "Terrier", "Toad", "Trooper", "Urchin",
];

/// Deterministic, display-only human-readable petname for an agent PID.
/// Applies dual independent 32-bit integer mixes to eliminate correlation across sequential sibling PIDs.
pub fn petname_for_pid(pid: u32) -> String {
    let p = pid as usize;
    let mut h1 = p.wrapping_mul(0x9E3779B9);
    h1 ^= h1 >> 16;
    let mut h2 = p.wrapping_mul(0x85EBCA6B);
    h2 ^= (h1 ^ (h2 >> 13)).wrapping_mul(0xC2B2AE35);
    h2 ^= h2 >> 16;

    let adj = ADJECTIVES[h1 % ADJECTIVES.len()];
    let noun = NOUNS[h2 % NOUNS.len()];
    format!("{adj}{noun}")
}

/// Helper to identify the current process's petname, honoring inherited petnames for nanoworkers.
pub fn current_agent_petname() -> String {
    std::env::var("AICHAT_AGENT_PETNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| petname_for_pid(std::process::id()))
}

/// Format a PID with its human-readable petname for display: "12345 (SwiftFalcon)".
/// If formatting the current process's PID and an inherited petname is set (e.g. for a nanoworker),
/// the inherited petname is displayed instead of computing from PID.
pub fn format_agent_pid(pid: u32) -> String {
    if pid == std::process::id() {
        if let Ok(inherited) = std::env::var("AICHAT_AGENT_PETNAME") {
            if !inherited.is_empty() {
                return format!("{pid} ({inherited})");
            }
        }
    }
    format!("{pid} ({})", petname_for_pid(pid))
}

/// Format a dialog trace block for rendering with hierarchical guide rails, asymmetric framing, and responsive soft-wrapping.
pub fn format_dialog_block(
    agent: &str,
    pid: u32,
    turn: usize,
    max_turns: usize,
    direction: DialogDirection,
    content: &str,
) -> String {
    let depth = current_agent_depth();
    let color = agent_color(agent);
    let colored_agent = color.bold().paint(agent).to_string();

    let outer_indent = ancestor_rails(depth);
    let active_rail = match depth {
        0 => "│ ",
        1 => "║ ",
        _ => "╏ ",
    };
    let active_rail_colored = color.bold().paint(active_rail).to_string();
    let line_prefix = format!("{outer_indent}{active_rail_colored} ");
    let prefix_visible_width = depth * 6 + 3;

    let term_width = get_terminal_width();
    let max_content_width = term_width.saturating_sub(prefix_visible_width).max(30);

    let (icon, dir_str, dir_color) = match direction {
        DialogDirection::Request => ("📥", "PROMPT SUBMITTED TO LLM", nu_ansi_term::Color::Cyan),
        DialogDirection::Response => ("📤", "RESPONSE FROM LLM", nu_ansi_term::Color::Green),
    };

    let pid_str = format_agent_pid(pid);
    let colored_pid_str = color.paint(&pid_str).to_string();
    let header_title = format!("{icon} [{colored_pid_str} {colored_agent} [turn {turn}/{max_turns}] {}]", dir_color.bold().paint(dir_str));
    let outer_indent_width = depth * 6;
    let title_vis_width = visible_width(&header_title);
    let top_prefix_width = outer_indent_width + 4; // for "┌── "
    let top_total_width = top_prefix_width + title_vis_width;

    let header_bar = if term_width > top_total_width + 2 {
        let dashes_count = term_width - top_total_width - 2;
        let dashes = "─".repeat(dashes_count);
        format!("{outer_indent}┌── {header_title} {dashes}")
    } else {
        format!("{outer_indent}┌── {header_title}")
    };

    let footer_prefix_width = outer_indent_width + 4; // for "└── "
    let footer_dashes_count = term_width.saturating_sub(footer_prefix_width + 1).max(10);
    let footer_dashes = "┄".repeat(footer_dashes_count);
    let footer_bar = format!("{outer_indent}└── {footer_dashes}");

    let mut indented_lines = Vec::new();
    for raw_line in content.lines() {
        if raw_line.is_empty() {
            indented_lines.push(line_prefix.clone());
        } else {
            let cont_indent = detect_continuation_indent(raw_line);
            let wrapped_chunks = wrap_ansi_line(raw_line, max_content_width, cont_indent);
            for chunk in wrapped_chunks {
                indented_lines.push(format!("{line_prefix}{chunk}"));
            }
        }
    }

    format!(
        "\n{header_bar}\n{}\n{footer_bar}",
        indented_lines.join("\n")
    )
}

/// Write a complete block or line of output to `/dev/tty` (or stderr) in a single atomic
/// write syscall, guaranteeing that concurrent subprocesses writing to the same terminal
/// cannot interleave between the output text and its trailing newline.
pub fn write_atomic_terminal_output(output: &str) {
    use std::io::Write;
    let mut buf = Vec::with_capacity(output.len() + 1);
    buf.extend_from_slice(output.as_bytes());
    if !buf.ends_with(b"\n") {
        buf.push(b'\n');
    }
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        let _ = tty.write_all(&buf);
        let _ = tty.flush();
    } else {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(&buf);
        let _ = stderr.flush();
    }
}

/// Emit a dialog trace block to /dev/tty (live terminal) or stderr.
/// Used for standalone invocations where no AgentLoopProgress channel is attached.
pub fn emit_dialog_block(
    agent: &str,
    pid: u32,
    turn: usize,
    max_turns: usize,
    direction: DialogDirection,
    content: &str,
) {
    let block = format_dialog_block(agent, pid, turn, max_turns, direction, content);
    if *IS_STDOUT_TERMINAL {
        use std::io::Write;
        let mut stderr = std::io::stderr().lock();
        let mut buf = Vec::with_capacity(block.len() + 1);
        buf.extend_from_slice(block.as_bytes());
        if !buf.ends_with(b"\n") {
            buf.push(b'\n');
        }
        let _ = stderr.write_all(&buf);
        let _ = stderr.flush();
    } else {
        write_atomic_terminal_output(&block);
    }
}

/// Check if agent loop debug mode is active via environment variable.
pub fn is_agent_loop_debug() -> bool {
    std::env::var("AICHAT_AGENT_LOOP_DEBUG")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

/// Format an event as a trace line for stderr output (unstyled fallback).
#[allow(dead_code)]
pub fn format_trace_event(event: &AgentLoopEvent, pid: u32) -> Option<String> {
    format_trace_event_styled(event, pid, None)
}

/// Format an event as a trace line for stderr output with colored agent labels and error styling.
pub fn format_trace_event_styled(
    event: &AgentLoopEvent,
    pid: u32,
    agent_label: Option<&str>,
) -> Option<String> {
    let pid_str = format_agent_pid(pid);
    let is_styled = agent_label.is_some();
    let agent_tag = match agent_label {
        Some(label) => {
            let color = agent_color(label);
            let colored_label = color.bold().paint(label);
            let colored_pid = color.paint(&pid_str);
            format!("{colored_label} {colored_pid}")
        }
        None => pid_str.clone(),
    };

    match event {
        AgentLoopEvent::TurnStart { turn, max_turns } => {
            Some(format!("{agent_tag} [turn {turn}/{max_turns}] starting"))
        }
        AgentLoopEvent::ToolStart { name, .. } => Some(format!("{agent_tag} calling: {name}")),
        AgentLoopEvent::ToolComplete {
            name,
            duration,
            success,
        } => {
            let status = if *success {
                if is_styled {
                    nu_ansi_term::Color::Green.paint("completed").to_string()
                } else {
                    "completed".to_string()
                }
            } else if is_styled {
                ERROR_COLOR.bold().paint("FAILED").to_string()
            } else {
                "FAILED".to_string()
            };
            Some(format!("{agent_tag} {name} {status} ({:.1}s)", duration.as_secs_f64()))
        }
        AgentLoopEvent::ToolBlocked { name, reason } => {
            if is_styled {
                let block_str = ERROR_COLOR.bold().paint("BLOCK");
                let reason_str = ERROR_COLOR.paint(reason.as_str());
                Some(format!("{agent_tag} {block_str} {name}: {reason_str}"))
            } else {
                Some(format!("{agent_tag} BLOCK {name}: {reason}"))
            }
        }
        AgentLoopEvent::SubAgentStart { agent_name, pid: sub_pid } => {
            let sub_pid_str = format_agent_pid(*sub_pid);
            if is_styled {
                let sub_color = agent_color(agent_name);
                let colored_sub = sub_color.bold().paint(agent_name);
                let colored_sub_pid = sub_color.paint(&sub_pid_str);
                Some(format!("{agent_tag} sub-agent {colored_sub} started (PID {colored_sub_pid})"))
            } else {
                Some(format!("{agent_tag} sub-agent {agent_name} started (PID {sub_pid_str})"))
            }
        }
        AgentLoopEvent::SubAgentComplete {
            agent_name,
            pid: sub_pid,
            duration,
            success,
        } => {
            let sub_pid_str = format_agent_pid(*sub_pid);
            let status = if *success {
                if is_styled {
                    nu_ansi_term::Color::Green.paint("completed").to_string()
                } else {
                    "completed".to_string()
                }
            } else if is_styled {
                ERROR_COLOR.bold().paint("FAILED").to_string()
            } else {
                "FAILED".to_string()
            };
            if is_styled {
                let sub_color = agent_color(agent_name);
                let colored_sub = sub_color.bold().paint(agent_name);
                let colored_sub_pid = sub_color.paint(&sub_pid_str);
                Some(format!(
                    "{agent_tag} sub-agent {colored_sub} {status} ({:.1}s, PID {colored_sub_pid})",
                    duration.as_secs_f64()
                ))
            } else {
                Some(format!(
                    "{agent_tag} sub-agent {agent_name} {status} ({:.1}s, PID {sub_pid_str})",
                    duration.as_secs_f64()
                ))
            }
        }
        AgentLoopEvent::PlanReceived { content } => {
            let preview = if content.len() > 60 {
                format!("{}...", &content[..57])
            } else {
                content.clone()
            };
            Some(format!("{agent_tag} plan: \"{preview}\""))
        }
        AgentLoopEvent::BudgetWarning { turn, max_turns } => {
            if is_styled {
                let warn = nu_ansi_term::Color::Yellow.paint(format!("budget warning: turn {turn}/{max_turns}"));
                Some(format!("{agent_tag} {warn}"))
            } else {
                Some(format!("{agent_tag} budget warning: turn {turn}/{max_turns}"))
            }
        }
        AgentLoopEvent::BudgetExhausted { max_turns } => {
            if is_styled {
                let msg = ERROR_COLOR.bold().paint(format!("budget exhausted at {max_turns} turns"));
                Some(format!("{agent_tag} {msg}"))
            } else {
                Some(format!("{agent_tag} budget exhausted at {max_turns} turns"))
            }
        }
        AgentLoopEvent::CostExhausted { cost, max_cost } => {
            if is_styled {
                let msg = ERROR_COLOR.bold().paint(format!("cost exhausted: ${cost:.4} exceeded ${max_cost:.4} limit"));
                Some(format!("{agent_tag} {msg}"))
            } else {
                Some(format!("{agent_tag} cost exhausted: ${cost:.4} exceeded ${max_cost:.4} limit"))
            }
        }
        AgentLoopEvent::LoopComplete => Some(format!("{agent_tag} done")),
        AgentLoopEvent::PolicyRuleMatched { name, outcome } => {
            Some(format!("{agent_tag} policy matched: {name} -> {outcome}"))
        }
        AgentLoopEvent::SafetyGatePassed { name, comparison } => {
            if is_styled {
                let allow_str = nu_ansi_term::Color::Green.bold().paint("ALLOW");
                Some(format!("{agent_tag} {allow_str} {name}: {comparison}"))
            } else {
                Some(format!("{agent_tag} ALLOW {name}: {comparison}"))
            }
        }
        AgentLoopEvent::RiskAssessmentStart { name, model } => {
            Some(format!("{agent_tag} assess-risk: evaluating {name} with {model}"))
        }
        AgentLoopEvent::RiskAssessmentComplete {
            name,
            tier,
            confidence,
            rationale,
        } => {
            let preview = if rationale.len() > 60 {
                format!("{}...", &rationale[..57])
            } else {
                rationale.clone()
            };
            let tier_display = if is_styled {
                if tier.to_lowercase().contains("block") || tier.starts_with("T3") {
                    ERROR_COLOR.bold().paint(tier.as_str()).to_string()
                } else if tier.starts_with("T1") || tier.to_lowercase().contains("safe") {
                    nu_ansi_term::Color::Green.paint(tier.as_str()).to_string()
                } else {
                    tier.clone()
                }
            } else {
                tier.clone()
            };
            if preview.is_empty() {
                Some(format!("{agent_tag} assess-risk: verdict for {name} -> {tier_display} ({confidence})"))
            } else {
                Some(format!("{agent_tag} assess-risk: verdict for {name} -> {tier_display} ({confidence}): \"{preview}\""))
            }
        }
        AgentLoopEvent::RiskAssessmentError { name, error } => {
            if is_styled {
                let err_lbl = ERROR_COLOR.bold().paint("error");
                let err_msg = ERROR_COLOR.paint(error.as_str());
                Some(format!("{agent_tag} assess-risk: {err_lbl} for {name}: {err_msg}"))
            } else {
                Some(format!("{agent_tag} assess-risk: error for {name}: {error}"))
            }
        }
        AgentLoopEvent::RiskAssessmentCacheHit {
            name,
            cached_floor,
            rationale,
        } => {
            if let Some(r) = rationale.as_deref().filter(|r| !r.is_empty()) {
                let preview = if r.len() > 60 {
                    format!("{}...", &r[..57])
                } else {
                    r.to_string()
                };
                Some(format!("{agent_tag} assess-risk: cache hit for {name} (floor: {cached_floor}): \"{preview}\""))
            } else {
                Some(format!("{agent_tag} assess-risk: cache hit for {name} (floor: {cached_floor})"))
            }
        }
        AgentLoopEvent::EscalationDispatched { name, target, reason } => {
            if is_styled {
                let esc_tag = ESCALATION_COLOR.bold().paint("escalation:");
                let desc = ESCALATION_COLOR.paint(format!("{name} -> {target} ({reason})"));
                Some(format!("{agent_tag} {esc_tag} {desc}"))
            } else {
                Some(format!("{agent_tag} escalation: {name} -> {target} ({reason})"))
            }
        }
        AgentLoopEvent::EscalationVerdictReceived { name, decision } => {
            if is_styled {
                let verdict_tag = ESCALATION_COLOR.bold().paint("escalation verdict:");
                let dec_colored = if decision.to_lowercase().contains("halt") || decision.to_lowercase().contains("block") {
                    ERROR_COLOR.bold().paint(decision.as_str())
                } else {
                    nu_ansi_term::Color::Green.bold().paint(decision.as_str())
                };
                Some(format!("{agent_tag} {verdict_tag} {name} -> {dec_colored}"))
            } else {
                Some(format!("{agent_tag} escalation verdict: {name} -> {decision}"))
            }
        }
        AgentLoopEvent::HumanPromptRequested {
            name,
            blast_radius,
            reason,
        } => {
            if is_styled {
                let req_tag = ESCALATION_COLOR.bold().paint("human authorization requested:");
                let details = ESCALATION_COLOR.paint(format!("{name} ({blast_radius}, {reason})"));
                Some(format!("{agent_tag} {req_tag} {details}"))
            } else {
                Some(format!("{agent_tag} human authorization requested: {name} ({blast_radius}, {reason})"))
            }
        }
        AgentLoopEvent::HumanVerdictReceived { name, decision } => {
            if is_styled {
                let verdict_tag = ESCALATION_COLOR.bold().paint("human verdict:");
                let dec_colored = if decision.to_lowercase().contains("halt") || decision.to_lowercase().contains("reject") {
                    ERROR_COLOR.bold().paint(decision.as_str())
                } else {
                    nu_ansi_term::Color::Green.bold().paint(decision.as_str())
                };
                Some(format!("{agent_tag} {verdict_tag} {name} -> {dec_colored}"))
            } else {
                Some(format!("{agent_tag} human verdict: {name} -> {decision}"))
            }
        }
        AgentLoopEvent::RollbackJournalRecorded { name, entry_id, entry } => {
            let base_line = format!("{agent_tag} rollback journal: recorded {name} ({entry_id})");
            if is_agent_loop_debug() && entry.is_some() {
                let e = entry.as_ref().unwrap();
                let target = e.target_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "-".to_string());
                let artifact = e.artifact_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "-".to_string());
                let undo = e.undo_command.as_deref().unwrap_or("-");
                let args_compact = serde_json::to_string(&e.args).unwrap_or_default();
                let args_preview = if args_compact.len() > 120 {
                    format!("{}...", &args_compact[..117])
                } else {
                    args_compact
                };
                Some(format!(
                    "{base_line}\n    target_path:   {target}\n    artifact_path: {artifact}\n    undo_command:  {undo}\n    args:          {args_preview}"
                ))
            } else {
                Some(base_line)
            }
        }
        AgentLoopEvent::PreflightReversibilityApplied {
            name,
            mechanism,
            stepped_down_to,
        } => {
            Some(format!(
                "{agent_tag} preflight remediation: {name} (via {mechanism} -> stepped down to {stepped_down_to})"
            ))
        }
        AgentLoopEvent::CapabilityBlocked { name, unwound } => {
            if is_styled {
                let block_str = ERROR_COLOR.bold().paint("BLOCK");
                let reason_str = ERROR_COLOR.paint(format!("read-only mask (mutating tool; unwound: {unwound})"));
                Some(format!("{agent_tag} {block_str} {name}: {reason_str}"))
            } else {
                Some(format!(
                    "{agent_tag} BLOCK {name}: read-only mask (mutating tool; unwound: {unwound})"
                ))
            }
        }
        AgentLoopEvent::DialogBlock { .. } => None,
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
    let payload = format!(
        "\x07\x1b]777;notify;{title};{message}\x07\x1b]9;{message}\x07\x1b]99;i=aichat;{message}\x1b\\"
    );
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        let _ = tty.write_all(payload.as_bytes());
        let _ = tty.flush();
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
        _ => "working",
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

/// Compute the elapsed seconds since the agent run began.
///
/// Reads `AICHAT_START_TIME_MS` from the process environment if available,
/// allowing child/sub-agent processes to measure seconds from the root orchestrator's start.
/// If not set or invalid, falls back to `snapshot.elapsed`.
pub fn get_trace_elapsed_seconds(snapshot: &AgentLoopSnapshot) -> f64 {
    if let Ok(val) = std::env::var("AICHAT_START_TIME_MS") {
        if let Ok(start_ms) = val.parse::<u128>() {
            if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                let now_ms = now.as_millis();
                if now_ms >= start_ms {
                    return (now_ms - start_ms) as f64 / 1000.0;
                }
            }
        }
    }
    snapshot.elapsed.as_secs_f64()
}

/// Format a trace event item with guide rails and a leftmost elapsed seconds timestamp.
///
/// Each trace event item has exactly one timestamp placed at the left (before the bracket).
/// Format: `+X.Xs  [<event content>]`
/// Continuation lines for wrapped or multi-line items indent past the timestamp and opening bracket.
pub fn format_trace_item_with_timestamp(
    rails: &str,
    line: &str,
    elapsed: f64,
    term_width: usize,
) -> String {
    let ts_raw = format!("+{:.1}s", elapsed);
    let ts_pad = if ts_raw.len() < 6 {
        format!("{:>6}", ts_raw)
    } else {
        ts_raw.clone()
    };
    let ts_styled = nu_ansi_term::Color::DarkGray.paint(&ts_pad).to_string();

    let first_prefix = format!("{rails}  {ts_styled}  [");
    let cont_spaces = " ".repeat(ts_pad.len() + 5);
    let continuation_indent = format!("{rails}{cont_spaces}");

    let prefix_width = visible_width(rails) + ts_pad.len() + 5;
    let max_line_width = term_width.saturating_sub(prefix_width + 2).max(20);

    // Fast path: Single line that fits entirely without wrapping.
    if !line.contains('\n') && visible_width(line) <= max_line_width {
        return format!("{first_prefix}{line}]");
    }

    let mut formatted = String::new();
    let lines_vec: Vec<&str> = line.lines().collect();

    for (sub_idx, sub_line) in lines_vec.iter().enumerate() {
        let wrapped_chunks = wrap_ansi_line(sub_line, max_line_width, &continuation_indent);
        for (chunk_idx, chunk) in wrapped_chunks.iter().enumerate() {
            if sub_idx == 0 && chunk_idx == 0 {
                formatted.push_str(&format!("{first_prefix}{chunk}"));
            } else if chunk_idx == 0 {
                formatted.push_str(&format!("\n{continuation_indent}{chunk}"));
            } else {
                formatted.push_str(&format!("\n{chunk}"));
            }
        }
    }

    formatted.push(']');
    formatted
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
        if let Some(line) = format_trace_event_styled(event, pid, Some(agent_label)) {
            *trace_header_printed = true;
            let depth = current_agent_depth();
            let rails = ancestor_rails(depth);
            let term_width = get_terminal_width();
            let elapsed_secs = get_trace_elapsed_seconds(snapshot);
            let formatted_line =
                format_trace_item_with_timestamp(&rails, &line, elapsed_secs, term_width);

            if *IS_STDOUT_TERMINAL {
                spinner.print_line(formatted_line)?;
            } else {
                write_atomic_terminal_output(&formatted_line);
            }
        }
    }

    // 1b. Dialog trace output (prompt submitted and response from LLM)
    //     Routed through the event loop so trace lines and dialog blocks maintain
    //     strict FIFO causal ordering with zero race conditions.
    if config.show_dialog {
        if let AgentLoopEvent::DialogBlock {
            agent,
            pid: d_pid,
            turn,
            max_turns,
            direction,
            content,
        } = event
        {
            let block = format_dialog_block(agent, *d_pid, *turn, *max_turns, *direction, content);
            if *IS_STDOUT_TERMINAL {
                spinner.print_line(block)?;
            } else {
                write_atomic_terminal_output(&block);
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

    /// Serializes tests that mutate the process-global `AICHAT_CAPABILITY_MASK`
    /// env var, so they don't race each other under the parallel test runner.
    static MASK_ENV_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    #[allow(dead_code)]
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
        assert!(!al.show_dialog);
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
        let pid_str = format_agent_pid(pid);
        let event = AgentLoopEvent::ToolComplete {
            name: "fs_write".to_string(),
            duration: Duration::from_millis(1234),
            success: true,
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert!(line.contains(&pid_str));
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
        assert!(line.contains(&pid_str));

        // A gate-blocked tool traces as BLOCK (not "completed") with its explicit comparison.
        let event = AgentLoopEvent::ToolBlocked {
            name: "fs_write".to_string(),
            reason: "risk disruptive > ceiling reversible".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} BLOCK fs_write: risk disruptive > ceiling reversible"));
        assert!(!line.contains("completed"));

        // Safety Events Formatting
        let event = AgentLoopEvent::PolicyRuleMatched {
            name: "fs_write".to_string(),
            outcome: "raise to destructive".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} policy matched: fs_write -> raise to destructive"));

        let event = AgentLoopEvent::SafetyGatePassed {
            name: "read_logs".to_string(),
            comparison: "risk safe <= ceiling destructive".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} ALLOW read_logs: risk safe <= ceiling destructive"));

        let event = AgentLoopEvent::SafetyGatePassed {
            name: "write_file".to_string(),
            comparison: "risk reversible (effective, via backup) <= ceiling reversible".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} ALLOW write_file: risk reversible (effective, via backup) <= ceiling reversible"));

        let event = AgentLoopEvent::SafetyGatePassed {
            name: "wipe_disk_reversible".to_string(),
            comparison: "risk disruptive (effective, reversible tool) <= ceiling disruptive".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} ALLOW wipe_disk_reversible: risk disruptive (effective, reversible tool) <= ceiling disruptive"));

        let event = AgentLoopEvent::SafetyGatePassed {
            name: "execute_command".to_string(),
            comparison: "risk destructive (effective, policy raise) <= ceiling catastrophic".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} ALLOW execute_command: risk destructive (effective, policy raise) <= ceiling catastrophic"));

        let event = AgentLoopEvent::RiskAssessmentStart {
            name: "fs_write".to_string(),
            model: "gemini:gemini-2.5-flash".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} assess-risk: evaluating fs_write with gemini:gemini-2.5-flash"));

        let event = AgentLoopEvent::RiskAssessmentComplete {
            name: "fs_write".to_string(),
            tier: "Destructive".to_string(),
            confidence: "High".to_string(),
            rationale: "Overwrites configuration files".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} assess-risk: verdict for fs_write -> Destructive (High): \"Overwrites configuration files\""));

        let event = AgentLoopEvent::RiskAssessmentError {
            name: "fs_write".to_string(),
            error: "connection timeout".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} assess-risk: error for fs_write: connection timeout"));

        let event = AgentLoopEvent::RiskAssessmentCacheHit {
            name: "fs_write".to_string(),
            cached_floor: "Destructive".to_string(),
            rationale: None,
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} assess-risk: cache hit for fs_write (floor: Destructive)"));

        let event_with_rat = AgentLoopEvent::RiskAssessmentCacheHit {
            name: "fs_write".to_string(),
            cached_floor: "Destructive".to_string(),
            rationale: Some("cached rationale".into()),
        };
        let line = format_trace_event(&event_with_rat, pid).unwrap();
        assert_eq!(line, format!("{pid_str} assess-risk: cache hit for fs_write (floor: Destructive): \"cached rationale\""));

        let event = AgentLoopEvent::EscalationDispatched {
            name: "wipe_disk".to_string(),
            target: "parent".to_string(),
            reason: "authority_exceeded".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} escalation: wipe_disk -> parent (authority_exceeded)"));

        let event = AgentLoopEvent::EscalationVerdictReceived {
            name: "wipe_disk".to_string(),
            decision: "Continue".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} escalation verdict: wipe_disk -> Continue"));

        let event = AgentLoopEvent::HumanPromptRequested {
            name: "drop_table".to_string(),
            blast_radius: "Catastrophic".to_string(),
            reason: "authority_exceeded".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} human authorization requested: drop_table (Catastrophic, authority_exceeded)"));

        let event = AgentLoopEvent::HumanVerdictReceived {
            name: "drop_table".to_string(),
            decision: "Halt".to_string(),
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} human verdict: drop_table -> Halt"));

        let event = AgentLoopEvent::RollbackJournalRecorded {
            name: "fs_write".to_string(),
            entry_id: "entry-abc-123".to_string(),
            entry: None,
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} rollback journal: recorded fs_write (entry-abc-123)"));

        // With debug active and entry present
        let prev_dbg = std::env::var("AICHAT_AGENT_LOOP_DEBUG").ok();
        std::env::set_var("AICHAT_AGENT_LOOP_DEBUG", "true");
        let debug_entry = crate::safety::RollbackJournalEntry {
            id: "entry-abc-123".to_string(),
            agent_id: "agent-1".to_string(),
            tree_id: "tree-1".to_string(),
            timestamp: 1000,
            tool: "fs_write".to_string(),
            args: json!({"path": "secret_file.txt", "content": "hello"}),
            working_dir: std::path::PathBuf::from("/repo"),
            shell: Some("/bin/bash".into()),
            target_path: Some(std::path::PathBuf::from("secret_file.txt")),
            artifact_path: Some(std::path::PathBuf::from("/tmp/backup-123.bak")),
            undo_command: Some("cp /tmp/backup-123.bak secret_file.txt".into()),
        };
        let event_debug = AgentLoopEvent::RollbackJournalRecorded {
            name: "fs_write".to_string(),
            entry_id: "entry-abc-123".to_string(),
            entry: Some(Box::new(debug_entry)),
        };
        let line_debug = format_trace_event(&event_debug, pid).unwrap();
        assert!(line_debug.contains("target_path:   secret_file.txt"));
        assert!(line_debug.contains("artifact_path: /tmp/backup-123.bak"));
        assert!(line_debug.contains("undo_command:  cp /tmp/backup-123.bak secret_file.txt"));
        assert!(line_debug.contains("args:          {\"content\":\"hello\",\"path\":\"secret_file.txt\"}")
            || line_debug.contains("args:          {\"path\":\"secret_file.txt\",\"content\":\"hello\"}"));

        match prev_dbg {
            Some(v) => std::env::set_var("AICHAT_AGENT_LOOP_DEBUG", v),
            None => std::env::remove_var("AICHAT_AGENT_LOOP_DEBUG"),
        }

        let event = AgentLoopEvent::CapabilityBlocked {
            name: "fs_write".to_string(),
            unwound: true,
        };
        let line = format_trace_event(&event, pid).unwrap();
        assert_eq!(line, format!("{pid_str} BLOCK fs_write: read-only mask (mutating tool; unwound: true)"));
    }

    #[test]
    fn test_format_trace_event_styled_colors_agent_labels_errors_and_escalations() {
        let pid = 12345u32;
        let label = "researcher";
        let color = agent_color(label);
        let pid_str = format_agent_pid(pid);
        let expected_agent_tag = format!("{} {}", color.bold().paint(label), color.paint(&pid_str));

        // 1. Tool start contains colored agent tag
        let event = AgentLoopEvent::ToolStart {
            name: "web_search".to_string(),
            id: None,
        };
        let line = format_trace_event_styled(&event, pid, Some(label)).unwrap();
        assert!(line.starts_with(&expected_agent_tag));
        assert!(line.contains("calling: web_search"));

        // 2. Failed tool execution is styled with ERROR_COLOR
        let event = AgentLoopEvent::ToolComplete {
            name: "web_search".to_string(),
            duration: Duration::from_millis(500),
            success: false,
        };
        let line = format_trace_event_styled(&event, pid, Some(label)).unwrap();
        let expected_failed = ERROR_COLOR.bold().paint("FAILED").to_string();
        assert!(line.contains(&expected_failed));

        // 3. Blocked tool is styled with ERROR_COLOR
        let event = AgentLoopEvent::ToolBlocked {
            name: "fs_write".to_string(),
            reason: "risk disruptive > ceiling reversible".to_string(),
        };
        let line = format_trace_event_styled(&event, pid, Some(label)).unwrap();
        let expected_block = ERROR_COLOR.bold().paint("BLOCK").to_string();
        let expected_reason = ERROR_COLOR.paint("risk disruptive > ceiling reversible").to_string();
        assert!(line.contains(&expected_block));
        assert!(line.contains(&expected_reason));

        // 4. Escalations are styled with ESCALATION_COLOR
        let event = AgentLoopEvent::EscalationDispatched {
            name: "wipe_disk".to_string(),
            target: "parent".to_string(),
            reason: "authority_exceeded".to_string(),
        };
        let line = format_trace_event_styled(&event, pid, Some(label)).unwrap();
        let expected_esc = ESCALATION_COLOR.bold().paint("escalation:").to_string();
        let expected_esc_desc = ESCALATION_COLOR.paint("wipe_disk -> parent (authority_exceeded)").to_string();
        assert!(line.contains(&expected_esc));
        assert!(line.contains(&expected_esc_desc));

        // 5. Escalation verdict with Halt has ESCALATION_COLOR verdict tag and ERROR_COLOR decision
        let event = AgentLoopEvent::EscalationVerdictReceived {
            name: "wipe_disk".to_string(),
            decision: "Halt".to_string(),
        };
        let line = format_trace_event_styled(&event, pid, Some(label)).unwrap();
        let expected_vtag = ESCALATION_COLOR.bold().paint("escalation verdict:").to_string();
        let expected_halt = ERROR_COLOR.bold().paint("Halt").to_string();
        assert!(line.contains(&expected_vtag));
        assert!(line.contains(&expected_halt));

        // 6. Human authorization prompt requested is styled with ESCALATION_COLOR
        let event = AgentLoopEvent::HumanPromptRequested {
            name: "drop_database".to_string(),
            blast_radius: "Catastrophic".to_string(),
            reason: "authority_exceeded".to_string(),
        };
        let line = format_trace_event_styled(&event, pid, Some(label)).unwrap();
        let expected_human_tag = ESCALATION_COLOR.bold().paint("human authorization requested:").to_string();
        assert!(line.contains(&expected_human_tag));
    }

    #[test]
    fn petname_generation_is_deterministic_and_spread() {
        let pet1 = petname_for_pid(42);
        let pet2 = petname_for_pid(42);
        assert_eq!(pet1, pet2, "petname must be strictly deterministic");

        let pet_next = petname_for_pid(43);
        assert_ne!(pet1, pet_next, "consecutive PIDs should produce distinct petnames");

        let formatted = format_agent_pid(12345);
        assert!(formatted.starts_with("12345 ("));
        assert!(formatted.ends_with(")"));
    }

    #[test]
    fn format_spinner_message_shows_tools_when_active() {
        let snapshot = AgentLoopSnapshot {
            current_turn: 2,
            max_turns: 20,
            active_tools: vec!["fs_write".to_string(), "web_search".to_string()],
            elapsed: Duration::from_secs(5),
            accumulated_cost: 0.0,
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
            accumulated_cost: 0.0,
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
        let config_inner = Config {
            functions: crate::function::Functions::init_from_declarations(vec![
                serde_json::from_value(json!({
                    "name": "loop_tool",
                    "description": "loops to itself",
                    "parameters": {"type": "object"},
                    "output": {"destination": "pipe", "target": "loop_tool"}
                }))
                .unwrap(),
            ]),
            ..Default::default()
        };
        let config: GlobalConfig = Arc::new(RwLock::new(config_inner));

        let result = detect_pipe_cycle(&config, "loop_tool");
        assert!(result.is_err());
    }

    #[test]
    fn pipe_cycle_detection_allows_linear_chain() {
        let config_inner = Config {
            functions: crate::function::Functions::init_from_declarations(vec![
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
            ]),
            ..Default::default()
        };
        let config: GlobalConfig = Arc::new(RwLock::new(config_inner));

        let result = detect_pipe_cycle(&config, "tool_a");
        assert!(result.is_ok());
    }

    // --- Backlog #5: output-routing edge cases (FR-1) ---

    #[test]
    fn pipe_cycle_detection_catches_multi_hop_cycle() {
        // A -> B -> C -> A must be rejected (existing tests only cover A->A and linear).
        let functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "hop_a",
                "description": "pipes to b",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": "hop_b"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "name": "hop_b",
                "description": "pipes to c",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": "hop_c"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "name": "hop_c",
                "description": "pipes back to a",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": "hop_a"}
            }))
            .unwrap(),
        ]);
        let config_inner = Config {
            functions,
            ..Default::default()
        };
        let config: GlobalConfig = Arc::new(RwLock::new(config_inner));

        let result = detect_pipe_cycle(&config, "hop_a");
        assert!(result.is_err(), "multi-hop cycle A->B->C->A must be detected");
    }

    #[test]
    fn pipe_cycle_detection_allows_multi_hop_linear_chain() {
        // A -> B -> C (terminating) must be accepted.
        let functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "lin_a",
                "description": "pipes to b",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": "lin_b"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "name": "lin_b",
                "description": "pipes to c",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": "lin_c"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "name": "lin_c",
                "description": "terminal, no pipe",
                "parameters": {"type": "object"}
            }))
            .unwrap(),
        ]);
        let config_inner = Config {
            functions,
            ..Default::default()
        };
        let config: GlobalConfig = Arc::new(RwLock::new(config_inner));

        let result = detect_pipe_cycle(&config, "lin_a");
        assert!(result.is_ok(), "linear chain A->B->C must be allowed");
    }

    #[test]
    fn capping_passes_result_at_exact_limit_boundary() {
        // A string value serializes to itself (value_to_string returns the raw string),
        // so a string of length == limit is exactly at the boundary and must pass unchanged.
        let limit = 128;
        let exact = json!("x".repeat(limit));
        let result = apply_capping(exact.clone(), "boundary_exact", limit);
        assert_eq!(result, exact, "content length == limit must pass unchanged");
    }

    #[test]
    fn capping_caps_result_one_byte_over_limit() {
        let limit = 128;
        let over = json!("x".repeat(limit + 1));
        let result = apply_capping(over, "boundary_over", limit);
        assert!(
            result.get("preview").is_some(),
            "content length == limit + 1 must be capped"
        );
        assert_eq!(result["total_bytes"], limit + 1);
        let preview = result["preview"].as_str().unwrap();
        assert_eq!(preview.len(), limit);

        if let Some(path) = result["full_output_path"].as_str() {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn file_routing_creates_missing_parent_directories() {
        // FR-1.4: nested, not-yet-existing parent dirs must be created.
        let unique = format!(
            "aichat-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let base = std::env::temp_dir().join(&unique);
        let nested = base.join("a").join("b").join("c");
        let target = nested.join("out.txt");
        // Precondition: none of these dirs exist yet.
        assert!(!base.exists());

        let routing = OutputRouting {
            destination: OutputDestination::File,
            path: Some(target.to_string_lossy().to_string()),
            target: None,
        };
        let output = json!("nested content");
        let result = route_to_file(&output, "nested_dir_test", &routing);

        assert!(
            result.get("written_to").is_some(),
            "write into freshly-created nested dirs must succeed, got {result:?}"
        );
        assert!(target.exists(), "parent directories must have been created");
        let content = std::fs::read_to_string(&target).unwrap();
        assert_eq!(content, "nested content");

        // Clean up the whole temp tree.
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn template_expansion_substitutes_numeric_timestamp() {
        let expanded = expand_path_template("/tmp/{{timestamp}}.out", "tool", None);
        assert!(!expanded.contains("{{"), "no unresolved markers");
        // Extract the timestamp segment and confirm it is all-numeric.
        let ts = expanded
            .trim_start_matches("/tmp/")
            .trim_end_matches(".out");
        assert!(!ts.is_empty(), "timestamp must expand to a value");
        assert!(
            ts.chars().all(|c| c.is_ascii_digit()),
            "timestamp must be numeric, got '{ts}'"
        );
    }

    #[test]
    fn template_expansion_resolves_all_variables_combined() {
        let expanded = expand_path_template(
            "/data/{{name}}/{{id}}-{{timestamp}}.{{ext}}",
            "combo_tool",
            Some("call-9"),
        );
        assert!(expanded.contains("combo_tool"));
        assert!(expanded.contains("call-9"));
        assert!(expanded.contains("txt"));
        assert!(!expanded.contains("{{"), "all variables resolved");
    }

    // --- Backlog #5: apply_output_routing dispatcher (runtime routing entry point) ---
    // These drive the async dispatcher directly (offline, no LLM), covering branches
    // the pure-helper tests don't reach: cycle abort via the dispatcher, file dispatch,
    // empty-target fallback, and the default context/capping path.

    #[tokio::test]
    async fn apply_output_routing_aborts_on_pipe_cycle() {
        // A tool that pipes to itself must yield a pipe_cycle_error through the
        // dispatcher (not just the detect_pipe_cycle helper).
        let functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "cyc_tool",
                "description": "self-pipe",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": "cyc_tool"}
            }))
            .unwrap(),
        ]);
        let config: GlobalConfig = Arc::new(RwLock::new(Config {
            functions,
            ..Default::default()
        }));

        let result = apply_output_routing(&config, "cyc_tool", json!("data"), 16384).await;
        assert_eq!(
            result["error"]["type"], "pipe_cycle_error",
            "self-piping tool must abort with pipe_cycle_error, got {result:?}"
        );
    }

    #[tokio::test]
    async fn apply_output_routing_dispatches_file_destination() {
        let unique = format!(
            "aichat-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let target = std::env::temp_dir().join(&unique).join("routed.txt");
        let functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "file_tool",
                "description": "writes to file",
                "parameters": {"type": "object"},
                "output": {"destination": "file", "path": target.to_string_lossy()}
            }))
            .unwrap(),
        ]);
        let config: GlobalConfig = Arc::new(RwLock::new(Config {
            functions,
            ..Default::default()
        }));

        let result = apply_output_routing(&config, "file_tool", json!("payload"), 16384).await;
        assert!(
            result.get("written_to").is_some(),
            "file destination must return a written_to confirmation, got {result:?}"
        );
        assert!(target.exists(), "file must actually be written");

        // Clean up the temp tree.
        if let Some(base) = target.parent() {
            let _ = std::fs::remove_dir_all(base);
        }
    }

    #[tokio::test]
    async fn apply_output_routing_pipe_with_empty_target_falls_back_to_capping() {
        // A pipe destination with an empty target string must not pipe; it falls
        // back to capping (here: small output passes through unchanged).
        let functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "empty_pipe",
                "description": "pipe with no target",
                "parameters": {"type": "object"},
                "output": {"destination": "pipe", "target": ""}
            }))
            .unwrap(),
        ]);
        let config: GlobalConfig = Arc::new(RwLock::new(Config {
            functions,
            ..Default::default()
        }));

        let small = json!("small output");
        let result = apply_output_routing(&config, "empty_pipe", small.clone(), 16384).await;
        assert_eq!(result, small, "empty-target pipe must fall back to capping/passthrough");
    }

    #[tokio::test]
    async fn apply_output_routing_default_context_caps_large_output() {
        // A tool with no routing declaration takes the default (context) path,
        // which caps output exceeding the limit.
        let config: GlobalConfig = Arc::new(RwLock::new(Config::default()));
        let large = json!("x".repeat(200));
        let result = apply_output_routing(&config, "unrouted_tool", large, 100).await;
        assert!(
            result.get("preview").is_some(),
            "unrouted large output must be capped on the default context path, got {result:?}"
        );
        if let Some(path) = result["full_output_path"].as_str() {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn value_to_string_handles_all_types() {
        assert_eq!(value_to_string(&json!("hello")), "hello");
        assert_eq!(value_to_string(&json!(null)), "");
        let obj_str = value_to_string(&json!({"a": 1}));
        assert!(obj_str.contains("\"a\""));
        assert!(obj_str.contains("1"));
    }

    // --- Backlog #5: circuit breaker & cost budget helpers ---

    fn err_result(tool: &str) -> ToolResult {
        ToolResult::new(
            ToolCall::new(tool.to_string(), json!({}), None),
            json!({"error": {"type": "x", "message": "boom"}}),
        )
    }

    fn ok_result(tool: &str) -> ToolResult {
        ToolResult::new(ToolCall::new(tool.to_string(), json!({}), None), json!("ok"))
    }

    #[test]
    fn circuit_breaker_trips_after_exactly_three_failures() {
        let mut counts = std::collections::HashMap::new();
        let mut tripped = std::collections::HashSet::new();

        // Two failures — not yet tripped.
        update_circuit_breaker(&[err_result("t")], &mut counts, &mut tripped);
        update_circuit_breaker(&[err_result("t")], &mut counts, &mut tripped);
        assert!(!tripped.contains("t"), "must not trip before 3 failures");
        assert_eq!(counts.get("t"), Some(&2));

        // Third failure — trips.
        update_circuit_breaker(&[err_result("t")], &mut counts, &mut tripped);
        assert!(tripped.contains("t"), "must trip on the 3rd consecutive failure");
    }

    #[test]
    fn circuit_breaker_success_resets_the_counter() {
        let mut counts = std::collections::HashMap::new();
        let mut tripped = std::collections::HashSet::new();

        update_circuit_breaker(&[err_result("t")], &mut counts, &mut tripped);
        update_circuit_breaker(&[err_result("t")], &mut counts, &mut tripped);
        // A success resets the count.
        update_circuit_breaker(&[ok_result("t")], &mut counts, &mut tripped);
        assert_eq!(counts.get("t"), None, "success must clear the failure counter");
        // A subsequent single failure must not trip (count restarts at 1).
        update_circuit_breaker(&[err_result("t")], &mut counts, &mut tripped);
        assert!(!tripped.contains("t"), "counter must have reset after the success");
        assert_eq!(counts.get("t"), Some(&1));
    }

    #[test]
    fn circuit_breaker_tracks_tools_independently() {
        let mut counts = std::collections::HashMap::new();
        let mut tripped = std::collections::HashSet::new();

        // Three failures for "a", one for "b", in mixed batches.
        update_circuit_breaker(&[err_result("a"), err_result("b")], &mut counts, &mut tripped);
        update_circuit_breaker(&[err_result("a")], &mut counts, &mut tripped);
        update_circuit_breaker(&[err_result("a")], &mut counts, &mut tripped);

        assert!(tripped.contains("a"), "a hit 3 failures and must be tripped");
        assert!(!tripped.contains("b"), "b had only 1 failure and must not be tripped");
    }

    #[test]
    fn cost_budget_exceeded_boundaries() {
        // Under budget.
        assert!(!cost_budget_exceeded(0.4, 0.5));
        // Over budget.
        assert!(cost_budget_exceeded(0.6, 0.5));
        // Exactly at budget — uses strict `>`, so equal is NOT exceeded.
        assert!(!cost_budget_exceeded(0.5, 0.5));
        // Zero budget means unlimited, even with high cost.
        assert!(!cost_budget_exceeded(1000.0, 0.0));
        // Negative budget is treated as unlimited.
        assert!(!cost_budget_exceeded(1000.0, -1.0));
    }


    // --- Backlog #5: cost parsing, event->state/notification mapping (FR-2) ---

    #[test]
    fn parse_cost_extracts_dollar_amount_from_stderr() {
        let stderr = "some trace line\nTokens: 100 input + 20 output | Estimated cost: $0.0123 (done)\nmore output";
        let cost = parse_cost_from_stderr(stderr);
        assert!((cost - 0.0123).abs() < 1e-9, "expected 0.0123, got {cost}");
    }

    #[test]
    fn parse_cost_returns_zero_when_marker_absent() {
        let stderr = "no cost marker here\njust regular trace output\n";
        assert_eq!(parse_cost_from_stderr(stderr), 0.0);
    }

    #[test]
    fn parse_cost_tolerates_malformed_amount() {
        // Marker present but the token after '$' is not a valid float — must not panic, returns 0.0.
        let stderr = "Estimated cost: $notanumber trailing";
        assert_eq!(parse_cost_from_stderr(stderr), 0.0);
        // Empty after marker (marker at end of line).
        let stderr2 = "Estimated cost: $";
        assert_eq!(parse_cost_from_stderr(stderr2), 0.0);
    }

    #[test]
    fn state_from_event_maps_all_variants() {
        use std::time::Duration;
        // Terminal / budget / cost states
        assert_eq!(state_from_event(&AgentLoopEvent::LoopComplete), "done");
        assert_eq!(
            state_from_event(&AgentLoopEvent::BudgetExhausted { max_turns: 20 }),
            "budget_exhausted"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::CostExhausted {
                cost: 1.0,
                max_cost: 0.5
            }),
            "cost_exhausted"
        );
        // Working states — every non-terminal event maps to "working"
        assert_eq!(
            state_from_event(&AgentLoopEvent::TurnStart {
                turn: 1,
                max_turns: 20
            }),
            "working"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::ToolStart {
                name: "t".into(),
                id: None
            }),
            "working"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::ToolComplete {
                name: "t".into(),
                duration: Duration::from_secs(1),
                success: true
            }),
            "working"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::ToolBlocked {
                name: "t".into(),
                reason: "policy_forbidden".into()
            }),
            "working"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::SubAgentStart {
                agent_name: "a".into(),
                pid: 1
            }),
            "working"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::SubAgentComplete {
                agent_name: "a".into(),
                pid: 1,
                duration: Duration::from_secs(1),
                success: true
            }),
            "working"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::PlanReceived {
                content: "c".into()
            }),
            "working"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::BudgetWarning {
                turn: 18,
                max_turns: 20
            }),
            "working"
        );
        assert_eq!(
            state_from_event(&AgentLoopEvent::DialogBlock {
                agent: "test".into(),
                pid: 1,
                turn: 1,
                max_turns: 20,
                direction: DialogDirection::Request,
                content: "prompt".into(),
            }),
            "working"
        );
    }

    #[test]
    fn test_dialog_block_fifo_event_ordering() {
        let (progress, mut rx) = AgentLoopProgress::live();
        progress.emit(AgentLoopEvent::TurnStart { turn: 1, max_turns: 20 });
        progress.emit(AgentLoopEvent::DialogBlock {
            agent: "test".into(),
            pid: 123,
            turn: 1,
            max_turns: 20,
            direction: DialogDirection::Request,
            content: "prompt 1".into(),
        });
        progress.emit(AgentLoopEvent::ToolStart {
            name: "slow_task".into(),
            id: None,
        });
        progress.emit(AgentLoopEvent::ToolComplete {
            name: "slow_task".into(),
            duration: std::time::Duration::from_millis(100),
            success: true,
        });
        progress.emit(AgentLoopEvent::TurnStart { turn: 2, max_turns: 20 });
        progress.emit(AgentLoopEvent::DialogBlock {
            agent: "test".into(),
            pid: 123,
            turn: 2,
            max_turns: 20,
            direction: DialogDirection::Request,
            content: "prompt 2".into(),
        });

        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        assert_eq!(events.len(), 6);
        assert!(matches!(events[0], AgentLoopEvent::TurnStart { turn: 1, .. }));
        assert!(matches!(events[1], AgentLoopEvent::DialogBlock { turn: 1, .. }));
        assert!(matches!(events[2], AgentLoopEvent::ToolStart { .. }));
        assert!(matches!(events[3], AgentLoopEvent::ToolComplete { .. }));
        assert!(matches!(events[4], AgentLoopEvent::TurnStart { turn: 2, .. }));
        assert!(matches!(events[5], AgentLoopEvent::DialogBlock { turn: 2, .. }));
    }

    #[test]
    fn notification_for_event_fires_only_on_terminal_events() {
        use std::time::Duration;
        // Terminal events produce a notification.
        assert!(notification_for_event(&AgentLoopEvent::LoopComplete).is_some());
        assert!(notification_for_event(&AgentLoopEvent::BudgetExhausted { max_turns: 20 }).is_some());
        assert!(notification_for_event(&AgentLoopEvent::CostExhausted {
            cost: 1.0,
            max_cost: 0.5
        })
        .is_some());
        // Non-terminal / working events do not.
        assert!(notification_for_event(&AgentLoopEvent::TurnStart {
            turn: 1,
            max_turns: 20
        })
        .is_none());
        assert!(notification_for_event(&AgentLoopEvent::ToolComplete {
            name: "t".into(),
            duration: Duration::from_secs(1),
            success: true
        })
        .is_none());
        assert!(notification_for_event(&AgentLoopEvent::BudgetWarning {
            turn: 18,
            max_turns: 20
        })
        .is_none());
    }

    // --- Backlog #5: sub-agent depth boundary (FR-3) ---

    #[tokio::test]
    async fn agent_depth_guard_rejects_at_exact_max() {
        // eval_agent_tool_subprocess bails when current_depth >= max_agent_depth,
        // BEFORE spawning any subprocess. Using max_agent_depth: 0 means the guard
        // fires at the default depth (0 >= 0) without touching the process-global
        // AICHAT_AGENT_DEPTH env var — hermetic, race-free under parallel tests,
        // and no child process / provider involved.
        let config = config_with_agent_loop("agent_loop:\n  max_agent_depth: 0\n");
        let call = ToolCall::new("some_agent".to_string(), json!({"task": "noop"}), None);

        let result = eval_agent_tool_subprocess(&config, &call).await;

        assert!(
            result.is_err(),
            "depth (0) >= max_agent_depth (0) must be rejected before spawning"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("depth") && msg.contains("maximum"),
            "error should explain the depth limit, got: {msg}"
        );
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

    // --- Backlog #6a: capability-mask gate ---

    fn config_with_modes() -> GlobalConfig {
        let functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "fs_cat",
                "description": "read a file",
                "parameters": {"type": "object"},
                "mode": "readonly"
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "name": "fs_write",
                "description": "write a file",
                "parameters": {"type": "object"},
                "mode": "mutating"
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "name": "mystery_tool",
                "description": "no mode declared",
                "parameters": {"type": "object"}
            }))
            .unwrap(),
        ]);
        Arc::new(RwLock::new(Config {
            functions,
            ..Default::default()
        }))
    }

    #[test]
    fn tool_safety_class_resolves_from_config() {
        use crate::function::SafetyClass;
        let config = config_with_modes();
        assert_eq!(tool_safety_class(&config, "fs_cat"), SafetyClass::Readonly);
        assert_eq!(tool_safety_class(&config, "fs_write"), SafetyClass::Mutating);
        assert_eq!(
            tool_safety_class(&config, "mystery_tool"),
            SafetyClass::Unclassified
        );
        // A tool not present in config at all is treated as unclassified.
        assert_eq!(
            tool_safety_class(&config, "not_in_config"),
            SafetyClass::Unclassified
        );
    }

    #[test]
    fn capability_gate_permits_everything_when_unmasked() {
        // With no AICHAT_CAPABILITY_MASK set (top-level process), nothing is denied.
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_CAPABILITY_MASK").ok();
        std::env::remove_var("AICHAT_CAPABILITY_MASK");

        let config = config_with_modes();
        assert!(capability_denied_result(&config, "fs_cat").is_none());
        assert!(capability_denied_result(&config, "fs_write").is_none());
        assert!(capability_denied_result(&config, "mystery_tool").is_none());

        // Restore whatever was there (normally nothing).
        if let Some(v) = prev {
            std::env::set_var("AICHAT_CAPABILITY_MASK", v);
        }
    }

    #[test]
    fn capability_gate_denies_mutating_and_unclassified_when_masked() {
        // Serialize the env-var manipulation via a process-wide guard so this
        // test does not race the unmasked test above under the parallel runner.
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_CAPABILITY_MASK").ok();
        std::env::set_var("AICHAT_CAPABILITY_MASK", "readonly");

        let config = config_with_modes();

        // Readonly tool and the internal _plan pseudo-tool are always permitted.
        assert!(capability_denied_result(&config, "fs_cat").is_none());
        assert!(capability_denied_result(&config, "_plan").is_none());

        // Mutating tool is denied with reason "mutating".
        let mutating = capability_denied_result(&config, "fs_write")
            .expect("mutating tool must be denied under a readonly mask");
        assert_eq!(mutating["error"]["type"], "capability_denied");
        assert_eq!(mutating["error"]["reason"], "mutating");

        // Unclassified tool is denied with reason "unclassified".
        let unclassified = capability_denied_result(&config, "mystery_tool")
            .expect("unclassified tool must be denied under a readonly mask");
        assert_eq!(unclassified["error"]["type"], "capability_denied");
        assert_eq!(unclassified["error"]["reason"], "unclassified");

        // Restore prior state.
        match prev {
            Some(v) => std::env::set_var("AICHAT_CAPABILITY_MASK", v),
            None => std::env::remove_var("AICHAT_CAPABILITY_MASK"),
        }
    }

    // --- Backlog #6b: authority ceiling gate ---

    fn config_with_tiers() -> GlobalConfig {
        // default_ceiling defaults to Destructive.
        let functions = crate::function::Functions::init_from_declarations(vec![
            serde_json::from_value(json!({
                "name": "read_logs", "description": "read", "parameters": {"type":"object"}, "risk": "safe"
            })).unwrap(),
            serde_json::from_value(json!({
                "name": "restart_svc", "description": "restart", "parameters": {"type":"object"}, "risk": "disruptive"
            })).unwrap(),
            serde_json::from_value(json!({
                "name": "drop_table", "description": "drop", "parameters": {"type":"object"}, "risk": "catastrophic"
            })).unwrap(),
            serde_json::from_value(json!({
                "name": "wipe_disk", "description": "wipe", "parameters": {"type":"object"}, "risk": "destructive"
            })).unwrap(),
            serde_json::from_value(json!({
                "name": "wipe_disk_reversible", "description": "wipe w/ backup", "parameters": {"type":"object"},
                "risk": "destructive", "reversible": true
            })).unwrap(),
            serde_json::from_value(json!({
                "name": "write_file", "description": "write w/ backup", "parameters": {"type":"object"},
                "risk": "disruptive", "reversible_via": "backup"
            })).unwrap(),
            serde_json::from_value(json!({
                "name": "mystery", "description": "no classification", "parameters": {"type":"object"}
            })).unwrap(),
        ]);
        Arc::new(RwLock::new(Config { functions, ..Default::default() }))
    }

    fn call(name: &str) -> ToolCall {
        ToolCall::new(name.to_string(), json!({}), None)
    }

    #[test]
    fn authority_gate_permits_within_ceiling_denies_above() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        // Default top-level ceiling = Destructive (no env var).
        std::env::remove_var("AICHAT_AUTHORITY_CEILING");
        let config = config_with_tiers();

        // Within ceiling: Safe and Disruptive permitted.
        assert!(authority_denied_result(&config, &call("read_logs"), None, None).is_none());
        assert!(authority_denied_result(&config, &call("restart_svc"), None, None).is_none());
        // Destructive == ceiling → permitted.
        assert!(authority_denied_result(&config, &call("wipe_disk"), None, None).is_none());
        // Catastrophic > Destructive → authority_exceeded.
        let denied = authority_denied_result(&config, &call("drop_table"), None, None)
            .expect("catastrophic must exceed a destructive ceiling");
        assert_eq!(denied["error"]["type"], "authority_exceeded");
        assert_eq!(
            denied["error"]["comparison"],
            "risk catastrophic > ceiling destructive"
        );
        // _plan always permitted.
        assert!(authority_denied_result(&config, &call("_plan"), None, None).is_none());

        match prev {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
    }

    #[test]
    fn authority_gate_unclassified_is_human_reserved_blocked() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        std::env::remove_var("AICHAT_AUTHORITY_CEILING");
        let config = config_with_tiers();

        // Unclassified → Human → exceeds any autonomous ceiling → blocked (pre-#6d).
        let denied = authority_denied_result(&config, &call("mystery"), None, None)
            .expect("unclassified tool is human-reserved and must be blocked");
        assert_eq!(denied["error"]["type"], "authority_exceeded");
        assert_eq!(
            denied["error"]["comparison"],
            "risk human (unclassified tool) > ceiling destructive"
        );

        match prev {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
    }

    #[test]
    fn authority_gate_proven_reversibility_lowers_requirement() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        // Lower the ceiling to Disruptive so a plain Destructive action is blocked
        // but a *proven-reversible* Destructive (needs only Disruptive) is allowed.
        std::env::set_var("AICHAT_AUTHORITY_CEILING", "disruptive");
        let config = config_with_tiers();

        // Plain destructive → needs Destructive > Disruptive ceiling → blocked.
        assert!(authority_denied_result(&config, &call("wipe_disk"), None, None).is_some());
        // Proven-reversible destructive → needs only Disruptive → permitted.
        assert!(
            authority_denied_result(&config, &call("wipe_disk_reversible"), None, None).is_none(),
            "proven-reversible destructive should drop to disruptive and fit the ceiling"
        );

        match prev {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
    }

    #[test]
    fn authority_gate_child_ceiling_from_env_lowers_authority() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        // Simulate a sub-agent granted only a Safe ceiling.
        std::env::set_var("AICHAT_AUTHORITY_CEILING", "safe");
        let config = config_with_tiers();

        // Safe permitted; anything above blocked for this restricted child.
        assert!(authority_denied_result(&config, &call("read_logs"), None, None).is_none());
        assert!(authority_denied_result(&config, &call("restart_svc"), None, None).is_some());

        match prev {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
    }

    #[test]
    fn safety_block_reason_recognizes_gate_denials_only() {
        // The gate denial types are recognized...
        for t in [
            "capability_denied",
            "authority_exceeded",
            "policy_forbidden",
            "risk_blocked",
            "escalation_halted",
            "escalation_reverted",
            "escalation_failed",
        ] {
            let v = json!({"error": {"type": t, "message": "x"}});
            assert_eq!(safety_block_reason(&v).as_deref(), Some(t));
        }
        // ...but a genuine tool execution error is NOT a "block" (the tool ran
        // and failed — that must still trace as a real completion/failure).
        let exec_err = json!({"error": {"type": "tool_execution_error", "message": "boom"}});
        assert_eq!(safety_block_reason(&exec_err), None);
        // A normal successful result is not a block.
        assert_eq!(safety_block_reason(&json!({"output": "ok"})), None);
        assert_eq!(safety_block_reason(&json!("DONE")), None);
    }

    // --- Backlog #6c: risk evaluator decision (pure, mock-verdict) ---

    fn verdict(
        tier: crate::function::BlastRadius,
        confidence: crate::safety::VerdictConfidence,
    ) -> crate::safety::RiskVerdict {
        crate::safety::RiskVerdict {
            tier,
            reversible: false,
            confidence,
            rationale: "test".into(),
            concerns: vec![],
        }
    }

    #[test]
    fn risk_verdict_permissive_high_confidence_within_ceiling_proceeds() {
        use crate::function::BlastRadius::*;
        use crate::safety::{AuthorityCeiling, RequiredAuthority, VerdictConfidence};
        // Base Disruptive, ceiling Destructive, verdict agrees (or lower) with
        // high confidence → the action proceeds (no denial).
        let base = RequiredAuthority::Tier(Disruptive);
        let ceiling = AuthorityCeiling::UpTo(Destructive);
        assert!(
            risk_denied_from_verdict("restart_svc", base, ceiling, &verdict(Safe, VerdictConfidence::High), false)
                .is_none(),
            "a permissive high-confidence verdict must not loosen but also must not block within ceiling"
        );
        assert!(risk_denied_from_verdict(
            "restart_svc",
            base,
            ceiling,
            &verdict(Disruptive, VerdictConfidence::High),
            false,
        )
        .is_none());
    }

    #[test]
    fn risk_verdict_raise_over_ceiling_blocks() {
        use crate::function::BlastRadius::*;
        use crate::safety::{AuthorityCeiling, RequiredAuthority, VerdictConfidence};
        // Base Disruptive fits a Disruptive ceiling, but the evaluator raises it
        // to Destructive → now over ceiling → risk_blocked.
        let base = RequiredAuthority::Tier(Disruptive);
        let ceiling = AuthorityCeiling::UpTo(Disruptive);
        let denied = risk_denied_from_verdict(
            "restart_svc",
            base,
            ceiling,
            &verdict(Destructive, VerdictConfidence::High),
            false,
        )
        .expect("a raised-over-ceiling verdict must block");
        assert_eq!(denied["error"]["type"], "risk_blocked");
    }

    #[test]
    fn risk_verdict_low_confidence_fails_toward_block_even_within_ceiling() {
        use crate::function::BlastRadius::*;
        use crate::safety::{AuthorityCeiling, RequiredAuthority, VerdictConfidence};
        // Clamped tier still fits the ceiling, but low confidence must not
        // authorize a non-Safe action (fail-toward, FR-6c.8).
        let base = RequiredAuthority::Tier(Disruptive);
        let ceiling = AuthorityCeiling::UpTo(Destructive);
        let denied = risk_denied_from_verdict(
            "restart_svc",
            base,
            ceiling,
            &verdict(Disruptive, VerdictConfidence::Low),
            false,
        )
        .expect("a low-confidence verdict must fail toward blocking");
        assert_eq!(denied["error"]["type"], "risk_blocked");
    }

    #[test]
    fn risk_verdict_cannot_loosen_a_human_base() {
        use crate::function::BlastRadius::*;
        use crate::safety::{AuthorityCeiling, RequiredAuthority, VerdictConfidence};
        // A Human base (unclassified / policy-forbid / catastrophic) can never be
        // permitted by any verdict; it is over every ceiling.
        let base = RequiredAuthority::Human;
        let ceiling = AuthorityCeiling::UpTo(Destructive);
        let denied = risk_denied_from_verdict(
            "drop_table",
            base,
            ceiling,
            &verdict(Safe, VerdictConfidence::High),
            false,
        )
        .expect("Human base must remain blocked regardless of a permissive verdict");
        assert_eq!(denied["error"]["type"], "risk_blocked");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn risk_evaluator_disabled_without_risk_model_is_noop() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_ROLES_DIR").ok();
        let empty_dir = crate::utils::temp_file("-test-empty-roles-", "");
        std::fs::create_dir_all(&empty_dir).unwrap();
        std::env::set_var("AICHAT_ROLES_DIR", &empty_dir);

        // No safety.risk_model configured → the overlay degrades to #6b (proceeds
        // here; the #6b gate already ran separately). A disruptive tool within a
        // Destructive default ceiling must NOT be blocked by the evaluator.
        let config = config_with_tiers();
        assert!(config.read().safety.risk_model.is_none());
        assert!(
            risk_evaluator_denied_result(&config, &call("restart_svc"), false, None, None)
                .await
                .is_none(),
            "with no risk_model the evaluator must be a no-op (degrade to #6b)"
        );

        match prev {
            Some(v) => std::env::set_var("AICHAT_ROLES_DIR", v),
            None => std::env::remove_var("AICHAT_ROLES_DIR"),
        }
        let _ = std::fs::remove_dir_all(&empty_dir);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn risk_evaluator_uses_role_model_when_safety_risk_model_unset() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_ROLES_DIR").ok();
        let roles_dir = crate::utils::temp_file("-test-roles-", "");
        std::fs::create_dir_all(&roles_dir).unwrap();
        let role_content = "---\nmodel: custom:evaluator-model\n---\nPrompt";
        std::fs::write(roles_dir.join("%assess-risk%.md"), role_content).unwrap();
        std::env::set_var("AICHAT_ROLES_DIR", &roles_dir);

        let config = config_with_tiers();
        assert!(config.read().safety.risk_model.is_none());

        let (progress, mut rx) = AgentLoopProgress::live();
        let _ = risk_evaluator_denied_result(&config, &call("restart_svc"), false, None, Some(&progress)).await;
        
        let mut start_ev = None;
        while let Ok(event) = rx.try_recv() {
            if let AgentLoopEvent::RiskAssessmentStart { model, .. } = event {
                start_ev = Some(model);
                break;
            }
        }
        match prev {
            Some(v) => std::env::set_var("AICHAT_ROLES_DIR", v),
            None => std::env::remove_var("AICHAT_ROLES_DIR"),
        }
        let _ = std::fs::remove_dir_all(&roles_dir);

        assert_eq!(
            start_ev.as_deref(),
            Some("custom:evaluator-model"),
            "risk evaluator must use model defined in %assess-risk% role"
        );
    }

    #[tokio::test]
    async fn risk_evaluator_safe_fastpath_skips_even_when_enabled() {
        // Even with a risk_model set, a Safe tool must skip the evaluator entirely
        // (FR-6c.6). We assert no denial AND that no model call is attempted: the
        // configured model id is bogus, so if it were called it would error — but
        // the fast-path returns before any call, so this is a clean no-op.
        let config = config_with_tiers();
        config.write().safety.risk_model = Some("nonexistent:model".into());
        assert!(
            risk_evaluator_denied_result(&config, &call("read_logs"), false, None, None)
                .await
                .is_none(),
            "Safe tools must skip the evaluator (fast-path), not error on a bogus model"
        );
    }

    #[tokio::test]
    async fn risk_cache_hit_blocks_without_calling_the_model() {
        // Pre-seed the cache with a Human/Catastrophic verdict for a specific action. With a
        // bogus risk_model configured, a cache MISS would attempt the model and
        // fail-toward; but a cache HIT must decide from the cached floor alone
        // (no model call). We assert it blocks via the cached floor and preserves rationale.
        let config = config_with_tiers();
        config.write().safety.risk_model = Some("nonexistent:model".into());
        let cache = std::sync::Arc::new(parking_lot::Mutex::new(crate::safety::RiskCache::new()));
        let c = call("restart_svc");
        cache.lock().raise(
            &c.name,
            &c.arguments,
            crate::safety::RiskVerdict {
                tier: crate::function::BlastRadius::Catastrophic,
                reversible: false,
                confidence: crate::safety::VerdictConfidence::High,
                rationale: "catastrophic restart".into(),
                concerns: vec![],
            },
        );

        let denied = risk_evaluator_denied_result(&config, &c, false, Some(&cache), None)
            .await
            .expect("a cached Human floor must block");
        assert_eq!(denied["error"]["type"], "risk_blocked");
        assert!(denied["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Evaluator rationale: catastrophic restart."));
    }

    #[tokio::test]
    async fn risk_cache_hit_within_ceiling_proceeds_without_model() {
        // A cached floor that still fits the ceiling lets the action proceed —
        // again without any model call (bogus model would otherwise error).
        // restart_svc is Disruptive; default ceiling is Destructive.
        let config = config_with_tiers();
        config.write().safety.risk_model = Some("nonexistent:model".into());
        let cache = std::sync::Arc::new(parking_lot::Mutex::new(crate::safety::RiskCache::new()));
        let c = call("restart_svc");
        cache.lock().raise(
            &c.name,
            &c.arguments,
            crate::safety::RiskVerdict {
                tier: crate::function::BlastRadius::Disruptive,
                reversible: true,
                confidence: crate::safety::VerdictConfidence::High,
                rationale: "disruptive restart".into(),
                concerns: vec![],
            },
        );

        assert!(
            risk_evaluator_denied_result(&config, &c, false, Some(&cache), None)
                .await
                .is_none(),
            "a cached in-ceiling floor should proceed without re-calling the model"
        );
    }

    #[test]
    fn format_tool_invocation_formats_flags_and_values() {
        let args = json!({"command": "ls -la", "force": true, "dry_run": false});
        let formatted = format_tool_invocation("execute_command", &args).unwrap();
        assert!(formatted.starts_with("execute_command"));
        assert!(formatted.contains("--command \"ls -la\""));
        assert!(formatted.contains("--force"));
        assert!(!formatted.contains("--dry-run"));
    }

    #[test]
    fn resolve_tool_implementation_handles_unknown_gracefully() {
        let config = config_with_tiers();
        let res = resolve_tool_implementation(&config, "nonexistent_tool_xyz", None);
        assert_eq!(res, crate::safety::ToolImplementation::Unknown);
    }

    #[test]
    fn evaluator_context_end_to_end_resolution() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_FUNCTIONS_DIR").ok();
        let tmp = crate::utils::temp_file("-test-tools-", "");
        let tools_dir = tmp.join("tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        let script_file = tools_dir.join("my_tool.sh");
        std::fs::write(
            &script_file,
            "#!/usr/bin/env bash\n# @describe Test tool.\n# @option --cmd! Command to run\nmain() { eval \"$argc_cmd\"; }\n",
        ).unwrap();
        std::env::set_var("AICHAT_FUNCTIONS_DIR", &tmp);

        let config = config_with_tiers();
        let tool_name = "my_tool";
        let call_args = json!({"cmd": "whoami"});
        let decl = find_tool_declaration(&config, tool_name);
        let impl_info = resolve_tool_implementation(&config, tool_name, None);
        assert!(matches!(impl_info, crate::safety::ToolImplementation::Script { .. }));
        let decl_ctx = crate::safety::extract_declaration_context(decl.as_ref(), impl_info.source());
        assert_eq!(decl_ctx.as_ref().unwrap().description, "Test tool.");
        let invocation = format_tool_invocation(tool_name, &call_args);
        assert_eq!(invocation.as_deref(), Some("my_tool --cmd \"whoami\""));
        let context = crate::safety::build_evaluator_context(
            tool_name,
            &call_args,
            false,
            "execute tool 'my_tool'",
            decl_ctx.as_ref(),
            Some(&impl_info),
            invocation.as_deref(),
            None,
        );
        let parsed: serde_json::Value = serde_json::from_str(&context).unwrap();
        assert_eq!(parsed["tool"], "my_tool");
        assert!(parsed["source"].as_str().unwrap().contains("eval"));
        assert!(parsed["script_path"].as_str().unwrap().contains("my_tool.sh"));

        match prev {
            Some(v) => std::env::set_var("AICHAT_FUNCTIONS_DIR", v),
            None => std::env::remove_var("AICHAT_FUNCTIONS_DIR"),
        }
    }

    #[tokio::test]
    async fn safety_events_emitted_during_gate_evaluation() {
        let (progress, mut rx) = AgentLoopProgress::live();
        let config = config_with_tiers();
        let temp_dir = crate::utils::temp_file("-test-policy-", "");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let policy_file = temp_dir.join("policy.yaml");
        std::fs::write(&policy_file, "rules:\n  - tool: restart_svc\n    raise: destructive\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&policy_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        config.write().safety.policy_file = Some(policy_file);

        let c = call("restart_svc");
        let result = authority_denied_result(&config, &c, Some(&progress), None);
        assert!(result.is_none());

        let ev1 = rx.try_recv().expect("must emit PolicyRuleMatched");
        assert!(matches!(ev1, AgentLoopEvent::PolicyRuleMatched { ref name, ref outcome } if name == "restart_svc" && outcome == "raise to destructive"));

        let ev2 = rx.try_recv().expect("must emit SafetyGatePassed");
        assert!(matches!(ev2, AgentLoopEvent::SafetyGatePassed { ref name, .. } if name == "restart_svc"));
    }

    #[tokio::test]
    async fn risk_cache_hit_emits_safety_event() {
        let (progress, mut rx) = AgentLoopProgress::live();
        let config = config_with_tiers();
        config.write().safety.risk_model = Some("nonexistent:model".into());
        let cache = std::sync::Arc::new(parking_lot::Mutex::new(crate::safety::RiskCache::new()));
        let c = call("restart_svc");
        cache.lock().raise(
            &c.name,
            &c.arguments,
            crate::safety::RiskVerdict {
                tier: crate::function::BlastRadius::Disruptive,
                reversible: true,
                confidence: crate::safety::VerdictConfidence::High,
                rationale: "disruptive".into(),
                concerns: vec![],
            },
        );

        let result = risk_evaluator_denied_result(&config, &c, false, Some(&cache), Some(&progress)).await;
        assert!(result.is_none());

        let ev = rx.try_recv().expect("must emit RiskAssessmentCacheHit");
        assert!(matches!(ev, AgentLoopEvent::RiskAssessmentCacheHit { ref name, ref cached_floor, ref rationale } if name == "restart_svc" && cached_floor == "disruptive" && rationale.as_deref() == Some("disruptive")));
    }

    #[tokio::test]
    async fn test_preflight_reversibility_allows_disruptive_tool_under_reversible_ceiling() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        std::env::set_var("AICHAT_AUTHORITY_CEILING", "reversible");
        let config = config_with_tiers();

        let (progress, mut rx) = AgentLoopProgress::live();
        let c = ToolCall::new("write_file".to_string(), json!({"path": "/tmp/test_preflight.txt"}), None);
        let mut proven_applied = false;
        let denied = authority_denied_result(&config, &c, Some(&progress), Some(&mut proven_applied));

        assert!(denied.is_none(), "preflight remediation should permit write_file under reversible ceiling");
        assert!(proven_applied, "proven_reversible_applied flag should be set to true");

        let mut emitted_preflight = false;
        let mut emitted_passed = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                AgentLoopEvent::PreflightReversibilityApplied { name, stepped_down_to, .. } => {
                    assert_eq!(name, "write_file");
                    assert_eq!(stepped_down_to, "reversible");
                    emitted_preflight = true;
                }
                AgentLoopEvent::SafetyGatePassed { name, comparison } => {
                    assert_eq!(name, "write_file");
                    assert_eq!(comparison, "risk reversible (effective, via backup) <= ceiling reversible");
                    emitted_passed = true;
                }
                _ => {}
            }
        }
        assert!(emitted_preflight, "must emit PreflightReversibilityApplied");
        assert!(emitted_passed, "must emit SafetyGatePassed");

        match prev {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
    }

    #[test]
    fn test_preflight_reversibility_fails_closed_when_stepped_down_still_exceeds_ceiling() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        // Ceiling is Safe. write_file is Disruptive -> stepped down to Reversible.
        // Reversible still exceeds Safe!
        std::env::set_var("AICHAT_AUTHORITY_CEILING", "safe");
        let config = config_with_tiers();

        let c = ToolCall::new("write_file".to_string(), json!({"path": "/tmp/test_preflight.txt"}), None);
        let mut proven_applied = false;
        let denied = authority_denied_result(&config, &c, None, Some(&mut proven_applied));

        assert!(denied.is_some(), "stepped down authority still exceeding ceiling must fail closed");
        let val = denied.unwrap();
        assert_eq!(val["error"]["type"], "authority_exceeded");
        assert_eq!(
            val["error"]["comparison"],
            "risk disruptive > ceiling safe"
        );
        assert!(!proven_applied, "proven_reversible_applied flag must remain false when remediation does not suffice");

        match prev {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
    }

    #[test]
    fn test_authority_denied_comparison_formats_reversibility_discount() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        // Ceiling is Reversible. wipe_disk_reversible is Destructive discounted to Disruptive.
        // Disruptive > Reversible -> blocked with reversibility-discounted label!
        std::env::set_var("AICHAT_AUTHORITY_CEILING", "reversible");
        let config = config_with_tiers();

        let c = ToolCall::new("wipe_disk_reversible".to_string(), json!({}), None);
        let denied = authority_denied_result(&config, &c, None, None);

        assert!(denied.is_some());
        let val = denied.unwrap();
        assert_eq!(val["error"]["type"], "authority_exceeded");
        assert_eq!(
            val["error"]["comparison"],
            "risk disruptive (effective, reversible tool) > ceiling reversible"
        );

        match prev {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
    }

    #[test]
    fn test_preflight_reversibility_cannot_bypass_policy_forbid() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev_ceiling = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        std::env::set_var("AICHAT_AUTHORITY_CEILING", "destructive");

        let config = config_with_tiers();
        let temp_dir = crate::utils::temp_file("-test-policy-", "");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let policy_file = temp_dir.join("policy.yaml");
        std::fs::write(&policy_file, "rules:\n  - tool: write_file\n    forbid: true\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&policy_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        config.write().safety.policy_file = Some(policy_file);

        let c = ToolCall::new("write_file".to_string(), json!({"path": "/tmp/test_preflight.txt"}), None);
        let mut proven_applied = false;
        let denied = authority_denied_result(&config, &c, None, Some(&mut proven_applied));

        assert!(denied.is_some(), "policy forbid must never be bypassed by preflight remediation");
        assert_eq!(denied.unwrap()["error"]["type"], "policy_forbidden");
        assert!(!proven_applied);

        match prev_ceiling {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
    }

    #[test]
    fn test_format_risk_token_all_cases() {
        use crate::function::{BlastRadius, StaticTier};
        use crate::safety::RequiredAuthority;

        // Unmodified
        assert_eq!(
            format_risk_token(StaticTier::Tier(BlastRadius::Safe), RequiredAuthority::Tier(BlastRadius::Safe), None),
            "risk safe"
        );
        assert_eq!(
            format_risk_token(StaticTier::Tier(BlastRadius::Disruptive), RequiredAuthority::Tier(BlastRadius::Disruptive), None),
            "risk disruptive"
        );

        // Preflight backup discount
        assert_eq!(
            format_risk_token(StaticTier::Tier(BlastRadius::Disruptive), RequiredAuthority::Tier(BlastRadius::Reversible), Some("via backup")),
            "risk reversible (effective, via backup)"
        );

        // Intrinsic reversibility discount
        assert_eq!(
            format_risk_token(StaticTier::Tier(BlastRadius::Destructive), RequiredAuthority::Tier(BlastRadius::Disruptive), Some("reversible tool")),
            "risk disruptive (effective, reversible tool)"
        );

        // Policy raise
        assert_eq!(
            format_risk_token(StaticTier::Tier(BlastRadius::Disruptive), RequiredAuthority::Tier(BlastRadius::Destructive), None),
            "risk destructive (effective, policy raise)"
        );

        // Unclassified tool
        assert_eq!(
            format_risk_token(StaticTier::Unclassified, RequiredAuthority::Human, None),
            "risk human (unclassified tool)"
        );

        // Human required
        assert_eq!(
            format_risk_token(StaticTier::Tier(BlastRadius::Destructive), RequiredAuthority::Human, None),
            "risk human (human approval required)"
        );
    }

    // --- Backlog #6d / Option B: supervisory escalation decisions ---

    #[test]
    fn supervisory_decision_permissive_high_confidence_within_ceiling_permits() {
        use crate::function::BlastRadius::*;
        use crate::safety::{AuthorityCeiling, RequiredAuthority, VerdictConfidence};

        let base = RequiredAuthority::Tier(Disruptive);
        let ceiling = AuthorityCeiling::UpTo(Destructive);
        let v = verdict(Disruptive, VerdictConfidence::High);

        let (req, permitted) = supervisory_verdict_decision(base, ceiling, Some(&v), false);
        assert_eq!(req, RequiredAuthority::Tier(Disruptive));
        assert!(permitted, "within ceiling with high confidence must be permitted");
    }

    #[test]
    fn supervisory_decision_raises_over_ceiling_does_not_permit() {
        use crate::function::BlastRadius::*;
        use crate::safety::{AuthorityCeiling, RequiredAuthority, VerdictConfidence};

        let base = RequiredAuthority::Tier(Disruptive);
        let ceiling = AuthorityCeiling::UpTo(Disruptive);
        let v = verdict(Destructive, VerdictConfidence::High);

        let (req, permitted) = supervisory_verdict_decision(base, ceiling, Some(&v), false);
        assert_eq!(req, RequiredAuthority::Tier(Destructive));
        assert!(!permitted, "raised over ceiling must not be permitted");
    }

    #[test]
    fn supervisory_decision_low_confidence_fails_toward_human() {
        use crate::function::BlastRadius::*;
        use crate::safety::{AuthorityCeiling, RequiredAuthority, VerdictConfidence};

        let base = RequiredAuthority::Tier(Disruptive);
        let ceiling = AuthorityCeiling::UpTo(Destructive);
        let v = verdict(Disruptive, VerdictConfidence::Low);

        let (req, permitted) = supervisory_verdict_decision(base, ceiling, Some(&v), false);
        assert_eq!(req, RequiredAuthority::Human);
        assert!(!permitted, "low confidence must fail toward Human and not be permitted");
    }

    #[test]
    fn supervisory_decision_without_verdict_uses_base() {
        use crate::function::BlastRadius::*;
        use crate::safety::{AuthorityCeiling, RequiredAuthority};

        let base = RequiredAuthority::Tier(Disruptive);
        let ceiling = AuthorityCeiling::UpTo(Destructive);

        let (req, permitted) = supervisory_verdict_decision(base, ceiling, None, false);
        assert_eq!(req, RequiredAuthority::Tier(Disruptive));
        assert!(permitted);

        let ceiling_safe = AuthorityCeiling::UpTo(Safe);
        let (_, permitted_safe) = supervisory_verdict_decision(base, ceiling_safe, None, false);
        assert!(!permitted_safe);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn handle_escalation_request_policy_forbid_returns_halt() {
        let _guard = MASK_ENV_LOCK.lock();
        let config = config_with_tiers();
        let temp_dir = crate::utils::temp_file("-test-orch-policy-", "");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let policy_file = temp_dir.join("policy.yaml");
        std::fs::write(&policy_file, "rules:\n  - tool: rm\n    forbid: true\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&policy_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        config.write().safety.policy_file = Some(policy_file);

        let hello = crate::safety::HelloMsg {
            agent_id: "child-coder".to_string(),
            depth: 1,
            capabilities: vec![],
        };
        let esc = crate::safety::EscalationMsg {
            id: "esc-1".to_string(),
            agent_id: "child-coder".to_string(),
            tree_id: "tree-test".to_string(),
            action: json!({"tool": "rm", "arguments": {"path": "/tmp/test"}}),
            reason: "needs rm".to_string(),
            enrichment: json!({}),
            blast_radius: crate::function::BlastRadius::Destructive,
            reversible: false,
            challenge: "chall".to_string(),
        };

        let verdict = handle_escalation_request(&config, &hello, esc).await;
        assert_eq!(verdict.decision, crate::safety::VerdictDecision::Halt);
        assert!(verdict.added_context.is_some());
        let err_obj = verdict.added_context.unwrap();
        assert_eq!(err_obj["error"]["type"], "policy_forbidden");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn handle_escalation_request_within_ceiling_approves() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev_ceiling = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        let prev_roles = std::env::var("AICHAT_ROLES_DIR").ok();
        let empty_dir = crate::utils::temp_file("-test-orch-empty-roles-", "");
        std::fs::create_dir_all(&empty_dir).unwrap();
        std::env::set_var("AICHAT_ROLES_DIR", &empty_dir);
        std::env::set_var("AICHAT_AUTHORITY_CEILING", "destructive");

        let config = config_with_tiers();
        let hello = crate::safety::HelloMsg {
            agent_id: "child-coder".to_string(),
            depth: 1,
            capabilities: vec![],
        };
        let esc = crate::safety::EscalationMsg {
            id: "esc-2".to_string(),
            agent_id: "child-coder".to_string(),
            tree_id: "tree-test".to_string(),
            action: json!({"tool": "write_file", "arguments": {"path": "/tmp/test.txt"}}),
            reason: "needs write".to_string(),
            enrichment: json!({}),
            blast_radius: crate::function::BlastRadius::Disruptive,
            reversible: false,
            challenge: "chall".to_string(),
        };

        let verdict = handle_escalation_request(&config, &hello, esc).await;
        assert_eq!(verdict.decision, crate::safety::VerdictDecision::Continue);

        match prev_ceiling {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
        match prev_roles {
            Some(v) => std::env::set_var("AICHAT_ROLES_DIR", v),
            None => std::env::remove_var("AICHAT_ROLES_DIR"),
        }
        let _ = std::fs::remove_dir_all(&empty_dir);
    }

    #[tokio::test]
    async fn handle_escalation_request_rejects_authority_exceeded_defense_in_depth() {
        let config = config_with_tiers();
        let hello = crate::safety::HelloMsg {
            agent_id: "child-coder".to_string(),
            depth: 1,
            capabilities: vec![],
        };
        let esc = crate::safety::EscalationMsg {
            id: "esc-auth-1".to_string(),
            agent_id: "child-coder".to_string(),
            tree_id: "tree-test".to_string(),
            action: json!({"tool": "write_file", "arguments": {"path": "/tmp/test.txt"}}),
            reason: "authority_exceeded".to_string(),
            enrichment: json!({}),
            blast_radius: crate::function::BlastRadius::Disruptive,
            reversible: false,
            challenge: "chall".to_string(),
        };

        let verdict = handle_escalation_request(&config, &hello, esc).await;
        assert_eq!(verdict.decision, crate::safety::VerdictDecision::Halt);
        assert!(verdict.added_context.is_some());
        let err_obj = verdict.added_context.unwrap();
        assert_eq!(err_obj["error"]["type"], "authority_exceeded");
    }

    #[tokio::test]
    async fn handle_escalation_request_rejects_capability_denied_defense_in_depth() {
        let config = config_with_tiers();
        let hello = crate::safety::HelloMsg {
            agent_id: "child-coder".to_string(),
            depth: 1,
            capabilities: vec![],
        };
        let esc = crate::safety::EscalationMsg {
            id: "esc-cap-1".to_string(),
            agent_id: "child-coder".to_string(),
            tree_id: "tree-test".to_string(),
            action: json!({"tool": "write_file", "arguments": {"path": "/tmp/test.txt"}}),
            reason: "capability_denied".to_string(),
            enrichment: json!({}),
            blast_radius: crate::function::BlastRadius::Disruptive,
            reversible: false,
            challenge: "chall".to_string(),
        };

        let verdict = handle_escalation_request(&config, &hello, esc).await;
        assert_eq!(verdict.decision, crate::safety::VerdictDecision::Halt);
        assert!(verdict.added_context.is_some());
        let err_obj = verdict.added_context.unwrap();
        assert_eq!(err_obj["error"]["type"], "capability_denied");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_eval_single_tool_child_process_authority_exceeded_fails_closed_without_escalating() {
        let _guard = MASK_ENV_LOCK.lock();
        let prev_ceiling = std::env::var("AICHAT_AUTHORITY_CEILING").ok();
        let prev_depth = std::env::var("AICHAT_AGENT_DEPTH").ok();

        // Set up child environment with depth 1 and safe ceiling
        std::env::set_var("AICHAT_AUTHORITY_CEILING", "safe");
        std::env::set_var("AICHAT_AGENT_DEPTH", "1");

        let config = config_with_tiers();
        // restart_svc is disruptive, exceeding the safe ceiling
        let call = ToolCall::new("restart_svc".to_string(), json!({}), None);

        let res = eval_single_tool(&config, &call, None, None).await.unwrap();
        assert_eq!(res["error"]["type"], "authority_exceeded");

        match prev_ceiling {
            Some(v) => std::env::set_var("AICHAT_AUTHORITY_CEILING", v),
            None => std::env::remove_var("AICHAT_AUTHORITY_CEILING"),
        }
        match prev_depth {
            Some(v) => std::env::set_var("AICHAT_AGENT_DEPTH", v),
            None => std::env::remove_var("AICHAT_AGENT_DEPTH"),
        }
    }

    #[test]
    fn test_truncate_payload_dialog_under_limit() {
        let text = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let result = truncate_payload_dialog(&text, 20, 20, false);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_payload_dialog_over_limit() {
        let text = (1..=100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let result = truncate_payload_dialog(&text, 20, 20, false);
        assert!(result.starts_with("line 1\nline 2\n"));
        assert!(result.ends_with("\nline 99\nline 100"));
        assert!(result.contains("... (payload truncated: 60 lines omitted) ..."));
        let lines: Vec<&str> = result.lines().collect();
        // 20 top + 1 truncation line + 20 bottom = 41 lines
        assert_eq!(lines.len(), 41);
        assert_eq!(lines[0], "line 1");
        assert_eq!(lines[19], "line 20");
        assert_eq!(lines[20], "... (payload truncated: 60 lines omitted) ...");
        assert_eq!(lines[21], "line 81");
        assert_eq!(lines[40], "line 100");
    }

    #[test]
    fn test_truncate_payload_dialog_no_truncate() {
        let text = (1..=100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let result = truncate_payload_dialog(&text, 20, 20, true);
        assert_eq!(result, text);
        assert!(!result.contains("omitted"));
    }

    #[test]
    fn test_format_messages_dialog_preserves_system_instructions() {
        use crate::client::{Message, MessageContent, MessageRole};
        // 60 lines of system instructions
        let sys_text = (1..=60).map(|i| format!("instruction {i}")).collect::<Vec<_>>().join("\n");
        // 60 lines of user payload
        let user_text = (1..=60).map(|i| format!("data {i}")).collect::<Vec<_>>().join("\n");
        let msgs = vec![
            Message::new(MessageRole::System, MessageContent::Text(sys_text.clone())),
            Message::new(MessageRole::User, MessageContent::Text(user_text)),
        ];
        let dialog = format_messages_dialog(&msgs, false);
        // System instructions must be preserved in full
        assert!(dialog.contains("[system]"));
        assert!(dialog.contains(&sys_text));
        // User payload must be truncated
        assert!(dialog.contains("... (payload truncated: 20 lines omitted) ..."));
        assert!(dialog.contains("data 1"));
        assert!(dialog.contains("data 20"));
        assert!(dialog.contains("data 41"));
        assert!(dialog.contains("data 60"));
        assert!(!dialog.contains("data 25"));
    }

    #[test]
    fn test_format_messages_dialog_no_truncate() {
        use crate::client::{Message, MessageContent, MessageRole};
        let sys_text = (1..=60).map(|i| format!("instruction {i}")).collect::<Vec<_>>().join("\n");
        let user_text = (1..=60).map(|i| format!("data {i}")).collect::<Vec<_>>().join("\n");
        let msgs = vec![
            Message::new(MessageRole::System, MessageContent::Text(sys_text.clone())),
            Message::new(MessageRole::User, MessageContent::Text(user_text)),
        ];
        let dialog = format_messages_dialog(&msgs, true);
        assert!(dialog.contains("[system]"));
        assert!(dialog.contains(&sys_text));
        assert!(!dialog.contains("omitted"));
        assert!(dialog.contains("data 25"));
        assert!(dialog.contains("data 60"));
    }

    #[test]
    fn test_format_messages_dialog_system_folding_on_turn_2() {
        use crate::client::{Message, MessageContent, MessageRole};
        let sys_text = (1..=60).map(|i| format!("instruction {i}")).collect::<Vec<_>>().join("\n");
        let user_text = "hello".to_string();
        let msgs = vec![
            Message::new(MessageRole::System, MessageContent::Text(sys_text.clone())),
            Message::new(MessageRole::User, MessageContent::Text(user_text)),
        ];
        let dialog = format_messages_dialog_with_turn(&msgs, false, 2);
        assert!(dialog.contains("instructions unchanged"));
        assert!(!dialog.contains("instruction 1"));
        assert!(dialog.contains("hello"));
    }

    #[test]
    fn test_format_messages_dialog_turn_delta_highlighting() {
        use crate::client::{Message, MessageContent, MessageContentToolCalls, MessageRole, ToolCall};
        use crate::function::ToolResult;
        let msgs = vec![
            Message::new(MessageRole::System, MessageContent::Text("sys".to_string())),
            Message::new(MessageRole::User, MessageContent::Text("initial prompt".to_string())),
            Message::new(MessageRole::Assistant, MessageContent::Text("thinking...".to_string())),
            Message::new(
                MessageRole::Tool,
                MessageContent::ToolCalls(MessageContentToolCalls {
                    text: "".to_string(),
                    sequence: false,
                    tool_results: vec![ToolResult {
                        call: ToolCall::new("web_search".to_string(), serde_json::json!({}), None),
                        output: serde_json::json!("search result payload"),
                    }],
                }),
            ),
        ];
        let dialog = format_messages_dialog_with_turn(&msgs, false, 2);
        let stripped = strip_ansi(&dialog);
        assert!(stripped.contains("[history: user]"));
        assert!(stripped.contains("[history: assistant]"));
        assert!(stripped.contains("[new: tool_result: web_search]"));
        assert!(stripped.contains("search result payload"));
    }

    #[test]
    fn test_format_llm_response_dims_blockquotes() {
        let raw_text = "> Prior quote from user\n> Second line of quote\n\nDirect response from assistant";
        let output = ChatCompletionsOutput {
            text: raw_text.to_string(),
            ..Default::default()
        };
        let formatted = format_llm_response(&output, &[], false);
        let expected_quoted_line = nu_ansi_term::Color::DarkGray.paint("> Prior quote from user").to_string();
        assert!(formatted.contains(&expected_quoted_line));
        assert!(formatted.contains("Direct response from assistant"));
    }

    #[test]
    fn test_format_messages_dialog_all_keywords_colored_and_corpus_dimmed() {
        use crate::client::{Message, MessageContent, MessageRole};
        let msgs = vec![
            Message::new(MessageRole::System, MessageContent::Text("sys instructions".to_string())),
            Message::new(MessageRole::User, MessageContent::Text("initial prompt".to_string())),
            Message::new(MessageRole::Assistant, MessageContent::Text("prior reply".to_string())),
            Message::new(MessageRole::User, MessageContent::Text("new prompt".to_string())),
        ];
        let dialog = format_messages_dialog_with_turn(&msgs, false, 2);

        // Keywords check
        let stripped = strip_ansi(&dialog);
        assert!(stripped.contains("[system: 1 lines instructions unchanged]") || stripped.contains("[system]"));
        assert!(stripped.contains("[history: user]"));
        assert!(stripped.contains("[history: assistant]"));
        assert!(stripped.contains("[new: user]"));

        // Colors check: history badge contains ESCALATION_COLOR and role color
        let expected_hist_user = format!("[{}: {}]", ESCALATION_COLOR.bold().paint("history"), nu_ansi_term::Color::Cyan.bold().paint("user"));
        let expected_hist_asst = format!("[{}: {}]", ESCALATION_COLOR.bold().paint("history"), nu_ansi_term::Color::Yellow.bold().paint("assistant"));
        assert!(dialog.contains(&expected_hist_user));
        assert!(dialog.contains(&expected_hist_asst));

        // Corpus dimming check: historic text is wrapped in DarkGray
        let expected_dimmed_user = nu_ansi_term::Color::DarkGray.paint("initial prompt").to_string();
        let expected_dimmed_asst = nu_ansi_term::Color::DarkGray.paint("prior reply").to_string();
        assert!(dialog.contains(&expected_dimmed_user));
        assert!(dialog.contains(&expected_dimmed_asst));

        // Active new text is plain foreground
        assert!(dialog.contains("new prompt"));
    }

    #[test]
    fn test_format_dialog_block_rails_and_asymmetric_framing() {
        let req = format_dialog_block("orchestrator", 1234, 1, 10, DialogDirection::Request, "hello world");
        assert!(req.contains("📥"));
        assert!(req.contains("PROMPT SUBMITTED TO LLM"));
        assert!(req.contains("│"));
        assert!(req.contains("hello world"));

        let resp = format_dialog_block("orchestrator", 1234, 1, 10, DialogDirection::Response, "model answer");
        assert!(resp.contains("📤"));
        assert!(resp.contains("RESPONSE FROM LLM"));
        assert!(resp.contains("│"));
        assert!(resp.contains("model answer"));
    }

    #[test]
    fn test_agent_color_assignment() {
        assert_eq!(agent_color("%assess-risk%"), nu_ansi_term::Color::Red);
        assert_eq!(agent_color("%functions%"), nu_ansi_term::Color::LightCyan);

        let c1 = agent_color("orchestrator");
        let c2 = agent_color("orchestrator");
        assert_eq!(c1, c2, "ephemeral color for same label must be stable in a process");

        std::env::set_var("AICHAT_AGENT_COLOR", "green");
        assert_eq!(agent_color("custom-agent"), nu_ansi_term::Color::Green);
        std::env::remove_var("AICHAT_AGENT_COLOR");
    }

    #[test]
    fn test_agent_depth_parsing() {
        let prev = std::env::var("AICHAT_AGENT_DEPTH").ok();
        std::env::remove_var("AICHAT_AGENT_DEPTH");
        assert_eq!(current_agent_depth(), 0);

        std::env::set_var("AICHAT_AGENT_DEPTH", "1");
        assert_eq!(current_agent_depth(), 1);

        std::env::set_var("AICHAT_AGENT_DEPTH", "3");
        assert_eq!(current_agent_depth(), 3);

        match prev {
            Some(v) => std::env::set_var("AICHAT_AGENT_DEPTH", v),
            None => std::env::remove_var("AICHAT_AGENT_DEPTH"),
        }
    }

    #[test]
    fn test_strip_ansi_and_visible_width() {
        let plain = "hello world";
        assert_eq!(strip_ansi(plain), "hello world");
        assert_eq!(visible_width(plain), 11);

        let styled = nu_ansi_term::Color::Red.bold().paint("alert!").to_string();
        assert_eq!(strip_ansi(&styled), "alert!");
        assert_eq!(visible_width(&styled), 6);

        let mixed = format!(
            "{} {}",
            nu_ansi_term::Color::Yellow.paint("⚡"),
            nu_ansi_term::Color::Magenta.bold().paint("[new: tool]")
        );
        assert_eq!(visible_width(&mixed), 14);
    }

    #[test]
    fn test_wrap_ansi_line_plain_text() {
        let text = "The quick brown fox jumps over the lazy dog and runs across the wide open meadow.";
        let chunks = wrap_ansi_line(text, 30, "    ");
        assert!(chunks.len() >= 3);
        for (i, chunk) in chunks.iter().enumerate() {
            assert!(visible_width(chunk) <= 30);
            if i > 0 {
                assert!(chunk.starts_with("    "));
            }
        }
    }

    #[test]
    fn test_wrap_ansi_line_with_ansi_styling() {
        let styled_lead = nu_ansi_term::Color::Yellow.bold().paint("⚡ [new: tool_result: web_search] ->").to_string();
        let body = "The Model Context Protocol (MCP) is an open standard and open-source framework developed by Anthropic in November 2024 to standardize AI system integrations.";
        let full = format!("{styled_lead} {body}");

        let chunks = wrap_ansi_line(&full, 60, "    ");
        assert!(chunks.len() >= 3);
        for (i, chunk) in chunks.iter().enumerate() {
            assert!(visible_width(chunk) <= 60);
            if i > 0 {
                assert!(chunk.starts_with("    "));
            }
        }
        assert!(chunks[0].contains('⚡'));
    }

    #[test]
    fn test_format_dialog_block_wraps_long_lines_with_guide_rails() {
        std::env::set_var("AICHAT_TERMINAL_WIDTH", "80");
        let long_line = "This is a very long line of output from a tool or an LLM response that contains more than one hundred characters and would ordinarily wrap to column zero breaking the rails.";
        let block = format_dialog_block("orchestrator", 100, 1, 10, DialogDirection::Response, long_line);

        for line in block.lines() {
            if line.is_empty() {
                continue;
            }
            assert!(line.starts_with("┌──") || line.starts_with("└──") || line.starts_with("\x1b[") || line.starts_with('│'));
            assert!(visible_width(line) <= 80, "line exceeded 80 columns: visible width was {}", visible_width(line));
        }
        std::env::remove_var("AICHAT_TERMINAL_WIDTH");
    }

    #[test]
    fn test_format_dialog_block_nested_child_rails_and_box_containment() {
        std::env::set_var("AICHAT_AGENT_DEPTH", "1");
        std::env::set_var("AICHAT_TERMINAL_WIDTH", "80");
        let long_line = "{\n  \"guidance\": \"Sub-agent 'coder' was blocked by its read-only permission mask when attempting 'fs_create' (reason: capability_denied). Pre-mutation entries were unwound.\"\n}";
        let block = format_dialog_block("coder", 200, 2, 20, DialogDirection::Request, long_line);

        for line in block.lines() {
            if line.is_empty() {
                continue;
            }
            assert!(visible_width(line) <= 80, "line exceeded 80 columns: visible width was {}", visible_width(line));
            assert!(line.contains('│'), "nested line missing parent rail: {}", line);
        }

        std::env::remove_var("AICHAT_AGENT_DEPTH");
        std::env::remove_var("AICHAT_TERMINAL_WIDTH");
    }

    #[tokio::test]
    async fn test_eval_tool_calls_parallel_preserves_all_results_without_dropping() {
        let config = config_with_tiers();
        let (progress, _rx) = AgentLoopProgress::live();
        let call = ToolCall::new("read_logs".to_string(), json!({}), None);
        let results = eval_tool_calls_parallel(
            &config,
            vec![call],
            create_abort_signal(),
            &progress,
            None,
        )
        .await
        .unwrap();

        // Must never be empty when calls was non-empty; results must be preserved for history
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].call.name, "read_logs");
    }

    #[test]
    fn test_agent_color_inherits_for_nano_workers() {
        assert_eq!(agent_color("nano-researcher"), agent_color("researcher"));
        assert_eq!(agent_color("nano-orchestrator"), agent_color("orchestrator"));
        assert_eq!(agent_color("nano-coder"), agent_color("coder"));
        assert_eq!(agent_color("nano-custom"), agent_color("custom"));
    }

    #[test]
    fn test_petname_generation_is_deterministic_and_spread() {
        let p1 = petname_for_pid(100);
        let p2 = petname_for_pid(100);
        assert_eq!(p1, p2, "petname generation must be deterministic");

        let mut petnames = std::collections::HashSet::new();
        for pid in 1000..1050 {
            petnames.insert(petname_for_pid(pid));
        }
        assert_eq!(petnames.len(), 50, "50 sequential PIDs should produce 50 unique petnames");
    }

    #[test]
    fn test_inherited_petname_formatting() {
        let current_pid = std::process::id();
        let default_formatted = format_agent_pid(current_pid);
        let default_petname = current_agent_petname();
        assert!(default_formatted.contains(&default_petname));

        std::env::set_var("AICHAT_AGENT_PETNAME", "nano-WittyFalcon-1");
        assert_eq!(current_agent_petname(), "nano-WittyFalcon-1");
        let formatted = format_agent_pid(current_pid);
        assert_eq!(formatted, format!("{current_pid} (nano-WittyFalcon-1)"));

        std::env::remove_var("AICHAT_AGENT_PETNAME");
        assert_eq!(current_agent_petname(), default_petname);
    }

    #[test]
    fn test_write_atomic_terminal_output_buffers() {
        write_atomic_terminal_output("test trace line without newline");
        write_atomic_terminal_output("test trace line with newline\n");
        write_atomic_terminal_output("line 1\nline 2\n");
    }

    #[test]
    fn test_get_trace_elapsed_seconds_fallback_and_env() {
        let snapshot = AgentLoopSnapshot {
            current_turn: 1,
            max_turns: 10,
            active_tools: vec![],
            elapsed: Duration::from_secs_f64(3.45),
            accumulated_cost: 0.0,
        };

        // Without env var, falls back to snapshot.elapsed
        std::env::remove_var("AICHAT_START_TIME_MS");
        let elapsed = get_trace_elapsed_seconds(&snapshot);
        assert!((elapsed - 3.45).abs() < 0.001);

        // With env var set to 2500ms ago
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let start_ms = now_ms.saturating_sub(2500);
        std::env::set_var("AICHAT_START_TIME_MS", start_ms.to_string());
        let elapsed_env = get_trace_elapsed_seconds(&snapshot);
        assert!((2.4..=3.0).contains(&elapsed_env), "expected ~2.5s elapsed, got {elapsed_env}");
        std::env::remove_var("AICHAT_START_TIME_MS");
    }

    #[test]
    fn test_format_trace_item_with_timestamp_single_line_leftmost() {
        let term_width = 80;
        let formatted = format_trace_item_with_timestamp("", "orchestrator [turn 1/20] starting", 0.5, term_width);
        // Single line output
        assert_eq!(formatted.lines().count(), 1);
        let stripped = strip_ansi(&formatted);
        // Leftmost timestamp format: `  +0.5s  [orchestrator [turn 1/20] starting]`
        assert!(stripped.starts_with("   +0.5s  ["));
        assert!(stripped.contains("orchestrator [turn 1/20] starting]"));
        assert!(stripped.ends_with(']'));
    }

    #[test]
    fn test_format_trace_item_with_timestamp_with_rails() {
        let term_width = 90;
        let rails = "│     ";
        let formatted = format_trace_item_with_timestamp(rails, "researcher calling: web_search", 12.3, term_width);
        assert_eq!(formatted.lines().count(), 1);
        let stripped = strip_ansi(&formatted);
        assert!(stripped.starts_with("│       +12.3s  ["));
        assert!(stripped.contains("researcher calling: web_search]"));
        assert!(stripped.ends_with(']'));
    }

    #[test]
    fn test_format_trace_item_with_timestamp_multiline_wrapping() {
        let term_width = 60;
        let long_line = "orchestrator plan: \"I will delegate TWO separate research tasks (call the researcher agent twice in parallel) to synthesize both results.\"";
        let formatted = format_trace_item_with_timestamp("", long_line, 1.2, term_width);
        let lines: Vec<&str> = formatted.lines().collect();
        assert!(lines.len() >= 2, "expected multiple lines, got {}", lines.len());

        // First line has the timestamp at left: `   +1.2s  [`
        let stripped_line0 = strip_ansi(lines[0]);
        assert!(stripped_line0.starts_with("   +1.2s  ["));

        // Continuation lines do NOT have the timestamp
        for (idx, line) in lines.iter().enumerate().skip(1) {
            let stripped = strip_ansi(line);
            assert!(!stripped.contains("+1.2s"), "line {} must not contain timestamp: {}", idx, line);
            // Indents past the timestamp and opening bracket (at least 11 spaces)
            assert!(stripped.starts_with("           "), "line {} must indent past timestamp: '{}'", idx, line);
        }

        // Final line closes with ']'
        assert!(lines.last().unwrap().ends_with(']'));
    }
}



