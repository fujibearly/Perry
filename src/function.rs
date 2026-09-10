use crate::{
    config::{Agent, Config, GlobalConfig},
    utils::*,
};

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
const PATH_SEP: &str = ";";
#[cfg(not(windows))]
const PATH_SEP: &str = ":";

#[allow(dead_code)]
pub fn eval_tool_calls(config: &GlobalConfig, calls: Vec<ToolCall>) -> Result<Vec<ToolResult>> {
    eval_tool_calls_with(calls, |call| call.eval(config))
}

#[allow(dead_code)]
pub fn eval_tool_calls_preserving_results(
    config: &GlobalConfig,
    calls: Vec<ToolCall>,
) -> Result<Vec<ToolResult>> {
    eval_tool_calls_with_options(calls, |call| call.eval(config), false)
}

/// Async version of `eval_tool_calls`. Executes tools sequentially but without
/// blocking the async runtime:
/// - MCP tools are called via async `.await` (no `block_in_place`)
/// - Shell-exec tools are wrapped in `tokio::task::spawn_blocking`
///
/// This is the foundation for parallel execution (Phase C) — the dispatch per
/// tool is async, even though this function currently processes them sequentially.
pub async fn eval_tool_calls_async(
    config: &GlobalConfig,
    calls: Vec<ToolCall>,
    _abort_signal: AbortSignal,
) -> Result<Vec<ToolResult>> {
    let mut output = vec![];
    if calls.is_empty() {
        return Ok(output);
    }
    let calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.")
    }
    let mut is_all_null = true;
    for call in calls {
        let mut result = match eval_single_tool_async(config, &call).await {
            Ok(result) => result,
            Err(_) => json!({
                "error": {
                    "type": "tool_execution_error",
                    "message": "The tool call failed. Fix its arguments or choose another tool."
                }
            }),
        };
        if result.is_null() {
            result = json!("DONE");
        } else {
            is_all_null = false;
        }
        output.push(ToolResult::new(call, result));
    }
    if is_all_null {
        output = vec![];
    }
    Ok(output)
}

/// Dispatch a single tool call asynchronously.
async fn eval_single_tool_async(config: &GlobalConfig, call: &ToolCall) -> Result<Value> {
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
            return crate::mcp::call_mcp_tool_async(&server_config, &original_name, arguments, timeout).await;
        }
    }

    // Route 2: Shell-exec tools (wrapped in spawn_blocking)
    let config = config.clone();
    let call = call.clone();
    tokio::task::spawn_blocking(move || call.eval_shell(&config))
        .await
        .map_err(|e| anyhow!("Tool task panicked: {e}"))?
}

#[allow(dead_code)]
fn eval_tool_calls_with<F>(calls: Vec<ToolCall>, eval: F) -> Result<Vec<ToolResult>>
where
    F: FnMut(&ToolCall) -> Result<Value>,
{
    eval_tool_calls_with_options(calls, eval, true)
}

fn eval_tool_calls_with_options<F>(
    mut calls: Vec<ToolCall>,
    mut eval: F,
    omit_all_null: bool,
) -> Result<Vec<ToolResult>>
where
    F: FnMut(&ToolCall) -> Result<Value>,
{
    let mut output = vec![];
    if calls.is_empty() {
        return Ok(output);
    }
    calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.")
    }
    let mut is_all_null = true;
    for call in calls {
        let mut result = match eval(&call) {
            Ok(result) => result,
            Err(_) => json!({
                "error": {
                    "type": "tool_execution_error",
                    "message": "The tool call failed. Fix its arguments or choose another tool."
                }
            }),
        };
        if result.is_null() {
            result = json!("DONE");
        } else {
            is_all_null = false;
        }
        output.push(ToolResult::new(call, result));
    }
    if omit_all_null && is_all_null {
        output = vec![];
    }
    Ok(output)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolResult {
    pub call: ToolCall,
    pub output: Value,
}

impl ToolResult {
    pub fn new(call: ToolCall, output: Value) -> Self {
        Self { call, output }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Functions {
    declarations: Vec<FunctionDeclaration>,
}

impl Functions {
    pub fn init(declarations_path: &Path) -> Result<Self> {
        let declarations: Vec<FunctionDeclaration> = if declarations_path.exists() {
            let ctx = || {
                format!(
                    "Failed to load functions at {}",
                    declarations_path.display()
                )
            };
            let content = fs::read_to_string(declarations_path).with_context(ctx)?;
            let mut decls: Vec<FunctionDeclaration> = serde_json::from_str(&content).with_context(ctx)?;
            for decl in &mut decls {
                if decl.agent {
                    decl.enrich_agent_permissions_schema();
                }
            }
            decls
        } else {
            vec![]
        };

        Ok(Self { declarations })
    }

    /// Append additional declarations (e.g., from MCP servers).
    pub fn extend(&mut self, mut declarations: Vec<FunctionDeclaration>) {
        for decl in &mut declarations {
            if decl.agent {
                decl.enrich_agent_permissions_schema();
            }
        }
        self.declarations.extend(declarations);
    }

    /// Create from an explicit list of declarations (for tests and programmatic use).
    #[allow(dead_code)]
    pub fn init_from_declarations(mut declarations: Vec<FunctionDeclaration>) -> Self {
        for decl in &mut declarations {
            if decl.agent {
                decl.enrich_agent_permissions_schema();
            }
        }
        Self { declarations }
    }

    pub fn find(&self, name: &str) -> Option<&FunctionDeclaration> {
        self.declarations.iter().find(|v| v.name == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.declarations.iter().any(|v| v.name == name)
    }

    pub fn declarations(&self) -> &[FunctionDeclaration] {
        &self.declarations
    }

    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
    #[serde(skip_serializing, default)]
    pub agent: bool,
    /// Output routing: where this tool's result goes after execution.
    /// Absent or null = context (default behavior).
    #[serde(skip_serializing, default)]
    pub output: Option<OutputRouting>,
    /// Safety mode (backlog #6a): whether this tool only reads (`readonly`) or
    /// may change state (`mutating`). Governance metadata — never serialized to
    /// the LLM. Absent means *unclassified* (see [`FunctionDeclaration::safety_class`]),
    /// which is treated as the most conservative disposition.
    #[serde(skip_serializing, default)]
    pub mode: Option<ToolMode>,
    /// Blast-radius tier (backlog #6b): the *impact* axis of this tool's actions,
    /// `Safe`..`Catastrophic`. Governance metadata — never serialized to the LLM.
    /// Absent means unclassified (reserved to humans, like an absent `mode`).
    #[serde(skip_serializing, default)]
    pub risk: Option<BlastRadius>,
    /// Intrinsic reversibility (backlog #6b): `Some(true)` if undoing this tool's
    /// action is inherent to the tool. The *proof* axis, orthogonal to `risk`.
    /// Proven reversibility lowers the authority required, never the tier.
    #[serde(skip_serializing, default)]
    pub reversible: Option<bool>,
    /// How reversibility is achieved when not intrinsic (backlog #6b), e.g.
    /// `"backup"`, `"staging"`, `"worktree"`. Informational; the actual rollback
    /// artifact is verified out-of-band (consumed from #9/#10).
    #[serde(skip_serializing, default)]
    #[allow(dead_code)] // consumed by #6c/#6d (rollback-artifact verification)
    pub reversible_via: Option<String>,
    /// Whether this tool executes as a one-shot utility worker ("nanoworker").
    /// Governance metadata — never serialized to the LLM.
    #[serde(skip_serializing, default)]
    pub nano: Option<bool>,
}

impl FunctionDeclaration {
    /// Resolve this tool's safety classification for capability-mask enforcement.
    ///
    /// - `Some(ToolMode::Readonly)` → the tool declares it only reads.
    /// - `Some(ToolMode::Mutating)` → the tool declares it changes state.
    /// - `None` (no `mode` declared) → **unclassified**: reserved to humans (for now),
    ///   the most conservative disposition. Unclassified tools are treated as at
    ///   least as restricted as `mutating` for masking purposes.
    pub fn safety_class(&self) -> SafetyClass {
        match self.mode {
            Some(ToolMode::Readonly) => SafetyClass::Readonly,
            Some(ToolMode::Mutating) => SafetyClass::Mutating,
            None => SafetyClass::Unclassified,
        }
    }

    /// Enrich an agent tool declaration with the optional `permissions` parameter schema.
    pub fn enrich_agent_permissions_schema(&mut self) {
        if !self.agent {
            return;
        }
        let props = self.parameters.properties.get_or_insert_with(IndexMap::new);
        if !props.contains_key("permissions") {
            if let Ok(schema) = serde_json::from_value::<JsonSchema>(json!({
                "type": "object",
                "description": "Optional permission contract for the delegated sub-agent. Cannot exceed orchestrator's own permissions.",
                "properties": {
                    "mask": {
                        "type": "string",
                        "enum": ["readonly", "mutating"],
                        "description": "Execution capability mask: readonly (safe reads only) or mutating (may alter state)"
                    },
                    "ceiling": {
                        "type": "string",
                        "enum": ["safe", "reversible", "disruptive", "destructive"],
                        "description": "Maximum autonomous blast-radius authority ceiling"
                    }
                }
            })) {
                props.insert("permissions".to_string(), schema);
            }
        }
        if !props.contains_key("permissions_mask") {
            if let Ok(schema) = serde_json::from_value::<JsonSchema>(json!({
                "type": "string",
                "enum": ["readonly", "mutating"],
                "description": "Flat fallback for permissions.mask"
            })) {
                props.insert("permissions_mask".to_string(), schema);
            }
        }
        if !props.contains_key("permissions_ceiling") {
            if let Ok(schema) = serde_json::from_value::<JsonSchema>(json!({
                "type": "string",
                "enum": ["safe", "reversible", "disruptive", "destructive"],
                "description": "Flat fallback for permissions.ceiling"
            })) {
                props.insert("permissions_ceiling".to_string(), schema);
            }
        }
    }

    /// Check whether this tool executes as a one-shot utility nanoworker.
    pub fn is_nano(&self) -> bool {
        self.nano.unwrap_or(false)
    }
}

/// Declared safety mode of a tool (backlog #6a).
///
/// Serialized form (in `functions.json`): `"readonly"` / `"mutating"`.
/// An *absent* mode is intentionally NOT one of these variants — see
/// [`SafetyClass::Unclassified`] — so that "no declaration" is distinguishable
/// from an explicit `mutating` and can carry the stricter reserved-to-humans policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolMode {
    /// The tool only reads state; safe to run under a read-only capability mask.
    Readonly,
    /// The tool may change state; blocked under a read-only capability mask.
    Mutating,
}

/// Resolved safety classification of a tool, including the "no declaration" case.
///
/// This is what capability-mask enforcement (backlog #6a) checks. It exists so
/// that an *undeclared* tool (`Unclassified`) is strictly more conservative than
/// an explicitly-`Mutating` one: undeclared tools are reserved to humans for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyClass {
    Readonly,
    Mutating,
    Unclassified,
}

impl SafetyClass {
    /// Whether a tool with this classification may run under a `readonly`
    /// capability mask. Only explicitly-`readonly` tools may; both `mutating`
    /// and `unclassified` are denied to masked (sub-agent) contexts.
    pub fn allowed_under_readonly_mask(&self) -> bool {
        matches!(self, SafetyClass::Readonly)
    }
}

/// Blast-radius tier (backlog #6b): the *impact* axis of an action, ordered by
/// increasing danger. Derives `Ord` so ceiling comparisons and the stricter-only
/// clamp are trivial `<=` / `max` operations.
///
/// Note: `Reversible` is a *tier name* for the low-impact-but-mutating band. It is
/// distinct from the orthogonal *proven-reversibility* boolean (`FunctionDeclaration::
/// reversible`) — the tier is the impact axis, the boolean is the proof axis; they
/// are combined only when computing required authority (see `src/safety.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BlastRadius {
    /// Radius 0: reads, idempotent queries — never changes state.
    Safe,
    /// Low-impact mutation.
    Reversible,
    /// Impactful but recoverable (e.g. restart a service).
    Disruptive,
    /// Irreversible loss if wrong (e.g. delete data without backup).
    Destructive,
    /// System- or fleet-level irreversible damage.
    Catastrophic,
}

impl BlastRadius {
    /// Lowercase string form, used for the `AICHAT_AUTHORITY_CEILING` env var
    /// (and matching the serde `rename_all = "lowercase"` wire form).
    pub fn as_str(&self) -> &'static str {
        match self {
            BlastRadius::Safe => "safe",
            BlastRadius::Reversible => "reversible",
            BlastRadius::Disruptive => "disruptive",
            BlastRadius::Destructive => "destructive",
            BlastRadius::Catastrophic => "catastrophic",
        }
    }

    /// Parse the lowercase string form (inverse of [`BlastRadius::as_str`]).
    pub fn from_str(s: &str) -> Option<BlastRadius> {
        match s {
            "safe" => Some(BlastRadius::Safe),
            "reversible" => Some(BlastRadius::Reversible),
            "disruptive" => Some(BlastRadius::Disruptive),
            "destructive" => Some(BlastRadius::Destructive),
            "catastrophic" => Some(BlastRadius::Catastrophic),
            _ => None,
        }
    }
}

/// The static (deterministic, pre-policy, pre-LLM) blast-radius classification of/// a tool, including the "no declaration" case.
///
/// Mirrors [`SafetyClass`] but on the 5-tier axis: an *undeclared* tool
/// (`Unclassified`) is strictly more conservative than any concrete tier — it is
/// reserved to humans (for now), sitting above every autonomous ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticTier {
    /// The tool declares a concrete blast radius.
    Tier(BlastRadius),
    /// No `risk`/`mode` declared (incl. MCP tools) — reserved to humans.
    Unclassified,
}

impl FunctionDeclaration {
    /// Resolve this tool's static blast-radius tier (backlog #6b), combining the
    /// explicit `risk` field with the legacy #6a `mode` field for back-compat:
    ///
    /// - explicit `risk` wins when present;
    /// - else legacy `mode`: `readonly` → `Safe`, `mutating` → `Disruptive`
    ///   (the conservative floor for "declared-mutating-but-no-finer-tier");
    /// - else (nothing declared) → `Unclassified` (reserved to humans).
    ///
    /// `readonly`→`Safe` and `mutating`→≥`Disruptive` is the compatibility rule
    /// from the spec (FR-6b.1) so #6a declarations keep working under #6b.
    pub fn static_tier(&self) -> StaticTier {
        if let Some(risk) = self.risk {
            return StaticTier::Tier(risk);
        }
        match self.mode {
            Some(ToolMode::Readonly) => StaticTier::Tier(BlastRadius::Safe),
            Some(ToolMode::Mutating) => StaticTier::Tier(BlastRadius::Disruptive),
            None => StaticTier::Unclassified,
        }
    }
}

/// Strongly-typed delegation permissions contract (Backlog #6d, FR-6d.18).
///
/// Allows an orchestrator / parent agent to provision execution capabilities
/// to a delegated sub-agent process at spawn time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DelegatedPermissions {
    #[serde(default)]
    pub mask: Option<String>, // "readonly" | "mutating"
    #[serde(default)]
    pub ceiling: Option<String>, // "safe" | "reversible" | "disruptive" | "destructive"
}

impl DelegatedPermissions {
    /// Parse delegated permissions from tool call arguments.
    ///
    /// Accepts:
    /// 1. Nested object: `permissions: { mask: "...", ceiling: "..." }`
    /// 2. Flat fallback: `permissions_mask: "..."`, `permissions_ceiling: "..."`
    ///
    /// If neither is present, returns `None`.
    pub fn parse_from_value(args: &Value) -> Option<Self> {
        let val: Value = match args {
            Value::String(s) => serde_json::from_str(s).ok()?,
            _ => args.clone(),
        };
        let obj = val.as_object()?;

        let mut perms: Option<DelegatedPermissions> = None;
        if let Some(p_val) = obj.get("permissions") {
            if let Ok(p) = serde_json::from_value::<DelegatedPermissions>(p_val.clone()) {
                if p.mask.is_some() || p.ceiling.is_some() {
                    perms = Some(p);
                }
            }
        }

        let flat_mask = obj
            .get("permissions_mask")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let flat_ceiling = obj
            .get("permissions_ceiling")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if flat_mask.is_some() || flat_ceiling.is_some() {
            let mut p = perms.unwrap_or_default();
            if p.mask.is_none() {
                p.mask = flat_mask;
            }
            if p.ceiling.is_none() {
                p.ceiling = flat_ceiling;
            }
            perms = Some(p);
        }

        perms
    }

    /// Validate requested permissions against parent capabilities, clamping unknown values to safe floor.
    ///
    /// Fails with an error if:
    /// - Child requests `mutating` when parent is `readonly`.
    /// - Child requests a ceiling higher than parent ceiling.
    /// - Child requests `catastrophic` ceiling (reserved to humans).
    ///
    /// Returns `(provisioned_mask, provisioned_ceiling)`.
    pub fn validate_against_parent(
        &self,
        parent_is_readonly: bool,
        parent_ceiling: crate::safety::AuthorityCeiling,
    ) -> Result<(String, crate::safety::AuthorityCeiling)> {
        let mask = match self.mask.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
            Some(ref m) if m == "mutating" => {
                if parent_is_readonly {
                    bail!("Parent runs under readonly capability mask and cannot provision mutating permission to sub-agent");
                }
                "mutating".to_string()
            }
            Some(ref m) if m == "readonly" => "readonly".to_string(),
            // Unknown or omitted values clamp closed to safe floor
            _ => "readonly".to_string(),
        };

        let ceiling = match self.ceiling.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
            Some(ref c) => match BlastRadius::from_str(c) {
                Some(BlastRadius::Catastrophic) => {
                    bail!("Catastrophic ceiling cannot be granted to autonomous sub-agents; reserved to humans");
                }
                Some(tier) => {
                    if tier > parent_ceiling.tier() {
                        bail!(
                            "Requested authority ceiling '{}' exceeds parent authority ceiling '{}'",
                            tier.as_str(),
                            parent_ceiling.tier().as_str()
                        );
                    }
                    crate::safety::AuthorityCeiling::UpTo(tier)
                }
                // Unknown values clamp closed to safe floor
                None => crate::safety::AuthorityCeiling::MINIMAL,
            },
            None => crate::safety::AuthorityCeiling::MINIMAL,
        };

        Ok((mask, ceiling))
    }

    /// Resolve effective permissions for a delegation call.
    /// If no permissions are specified, defaults strictly to the safe floor (`readonly`, `safe`).
    pub fn resolve_for_call(
        args: &Value,
        parent_is_readonly: bool,
        parent_ceiling: crate::safety::AuthorityCeiling,
    ) -> Result<(String, crate::safety::AuthorityCeiling)> {
        let requested = Self::parse_from_value(args).unwrap_or_default();
        requested.validate_against_parent(parent_is_readonly, parent_ceiling)
    }
}

/// Routing declaration for a tool's output.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct OutputRouting {
    /// Destination: "context" (default), "file", or "pipe".
    pub destination: OutputDestination,
    /// File path template (for "file" destination). Supports {{name}}, {{id}}, {{timestamp}}, {{ext}}.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Target tool name (for "pipe" destination).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Output destination for a tool result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputDestination {
    #[default]
    Context,
    File,
    Pipe,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JsonSchema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<IndexMap<String, JsonSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<JsonSchema>>,
    #[serde(rename = "anyOf", skip_serializing_if = "Option::is_none")]
    pub any_of: Option<Vec<JsonSchema>>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_value: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl JsonSchema {
    pub fn is_empty_properties(&self) -> bool {
        match &self.properties {
            Some(v) => v.is_empty(),
            None => true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
    pub id: Option<String>,
}

type CallConfig = (String, String, Vec<String>, HashMap<String, String>);

impl ToolCall {
    pub fn dedup(calls: Vec<Self>) -> Vec<Self> {
        let mut new_calls = vec![];
        let mut seen_ids = HashSet::new();

        for call in calls.into_iter().rev() {
            if let Some(id) = &call.id {
                if !seen_ids.contains(id) {
                    seen_ids.insert(id.clone());
                    new_calls.push(call);
                }
            } else {
                new_calls.push(call);
            }
        }

        new_calls.reverse();
        new_calls
    }

    pub fn new(name: String, arguments: Value, id: Option<String>) -> Self {
        Self {
            name,
            arguments,
            id,
        }
    }

    pub fn eval(&self, config: &GlobalConfig) -> Result<Value> {
        // Check if this is an MCP-sourced tool — route to MCP bridge if so
        #[cfg(feature = "mcp")]
        {
            let config_read = config.read();
            if let Some(entry) = config_read.mcp_tools.get(&self.name) {
                let server_name = entry.server_name.clone();
                let original_name = entry.original_name.clone();
                let server_config = config_read
                    .mcp_servers
                    .iter()
                    .find(|s| s.name == server_name)
                    .cloned();
                drop(config_read); // Release lock before blocking call

                let server_config = match server_config {
                    Some(c) => c,
                    None => bail!("MCP server config '{}' not found for tool '{}'", server_name, self.name),
                };

                let arguments = if self.arguments.is_object() {
                    self.arguments.clone()
                } else if let Some(args_str) = self.arguments.as_str() {
                    serde_json::from_str(args_str).unwrap_or_else(|_| self.arguments.clone())
                } else {
                    self.arguments.clone()
                };

                let timeout = std::time::Duration::from_secs(server_config.timeout);
                return crate::mcp::call_mcp_tool(&server_config, &original_name, arguments, timeout);
            }
        }

        self.eval_shell(config)
    }

    /// Execute this tool call via shell-exec only (no MCP routing).
    ///
    /// Used by the async parallel dispatch path where MCP and agent routing are
    /// handled separately before falling through to this method.
    pub fn eval_shell(&self, config: &GlobalConfig) -> Result<Value> {
        let (call_name, cmd_name, mut cmd_args, mut envs) = match &config.read().agent {
            Some(agent) => self.extract_call_config_from_agent(config, agent)?,
            None => self.extract_call_config_from_config(config)?,
        };

        if let Ok(start_ms) = std::env::var("AICHAT_START_TIME_MS") {
            envs.insert("AICHAT_START_TIME_MS".into(), start_ms);
        }

        let is_nano_tool = config
            .read()
            .agent
            .as_ref()
            .and_then(|a| a.functions().find(&self.name))
            .map(|f| f.is_nano())
            .unwrap_or_else(|| {
                config
                    .read()
                    .functions
                    .find(&self.name)
                    .map(|f| f.is_nano())
                    .unwrap_or(false)
            });

        let invoking_agent = config
            .read()
            .agent
            .as_ref()
            .map(|a| a.name().to_string())
            .or_else(|| {
                std::env::var("AICHAT_INVOKING_AGENT")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                std::env::var("AICHAT_AGENT_NAME")
                    .ok()
                    .and_then(|name| {
                        let clean = name.strip_prefix("nano-").unwrap_or(&name).to_string();
                        if clean.is_empty() || clean == "aichat" {
                            None
                        } else {
                            Some(clean)
                        }
                    })
            })
            .unwrap_or_default();

        if is_nano_tool && !invoking_agent.is_empty() {
            envs.insert("AICHAT_INVOKING_AGENT".into(), invoking_agent.clone());
            envs.insert("AICHAT_AGENT_NAME".into(), format!("nano-{}", self.name));
            let current_depth = crate::agent_loop::current_agent_depth();
            envs.insert("AICHAT_AGENT_DEPTH".into(), (current_depth + 1).to_string());

            use std::sync::atomic::{AtomicUsize, Ordering};
            static NANO_COUNTER: AtomicUsize = AtomicUsize::new(0);
            let seq = NANO_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
            let parent_petname = crate::agent_loop::current_agent_petname();
            envs.insert("AICHAT_AGENT_PETNAME".into(), format!("nano-{parent_petname}-{seq}"));

            let color_name = crate::agent_loop::current_agent_color_name(&invoking_agent);
            envs.insert("AICHAT_AGENT_COLOR".into(), color_name.to_string());
        }

        let json_data = if self.arguments.is_object() {
            self.arguments.clone()
        } else if let Some(arguments) = self.arguments.as_str() {
            let arguments: Value = serde_json::from_str(arguments).map_err(|_| {
                anyhow!("The call '{call_name}' has invalid arguments: {arguments}")
            })?;
            arguments
        } else {
            bail!(
                "The call '{call_name}' has invalid arguments: {}",
                self.arguments
            );
        };

        cmd_args.push(json_data.to_string());

        let output = match run_llm_function(cmd_name, cmd_args, envs)? {
            Some(contents) => serde_json::from_str(&contents)
                .ok()
                .unwrap_or_else(|| json!({"output": contents})),
            None => Value::Null,
        };

        Ok(output)
    }

    fn extract_call_config_from_agent(
        &self,
        config: &GlobalConfig,
        agent: &Agent,
    ) -> Result<CallConfig> {
        let function_name = self.name.clone();
        match agent.functions().find(&function_name) {
            Some(function) => {
                let agent_name = agent.name().to_string();
                if function.agent {
                    Ok((
                        format!("{agent_name}-{function_name}"),
                        agent_name,
                        vec![function_name],
                        agent.variable_envs(),
                    ))
                } else {
                    Ok((
                        function_name.clone(),
                        function_name,
                        vec![],
                        Default::default(),
                    ))
                }
            }
            None => self.extract_call_config_from_config(config),
        }
    }

    fn extract_call_config_from_config(&self, config: &GlobalConfig) -> Result<CallConfig> {
        let function_name = self.name.clone();
        match config.read().functions.contains(&function_name) {
            true => Ok((
                function_name.clone(),
                function_name,
                vec![],
                Default::default(),
            )),
            false => bail!("Unexpected call: {function_name} {}", self.arguments),
        }
    }
}

pub fn run_llm_function(
    cmd_name: String,
    cmd_args: Vec<String>,
    mut envs: HashMap<String, String>,
) -> Result<Option<String>> {
    let prompt = format!("Call {cmd_name} {}", cmd_args.join(" "));

    let mut bin_dirs: Vec<PathBuf> = vec![];
    if cmd_args.len() > 1 {
        let dir = Config::agent_functions_dir(&cmd_name).join("bin");
        if dir.exists() {
            bin_dirs.push(dir);
        }
    }
    bin_dirs.push(Config::functions_bin_dir());
    let current_path = std::env::var("PATH").context("No PATH environment variable")?;
    let prepend_path = bin_dirs
        .iter()
        .map(|v| format!("{}{PATH_SEP}", v.display()))
        .collect::<Vec<_>>()
        .join("");
    envs.insert("PATH".into(), format!("{prepend_path}{current_path}"));

    let temp_file = temp_file("-eval-", "");
    envs.insert("LLM_OUTPUT".into(), temp_file.display().to_string());

    #[cfg(windows)]
    let cmd_name = polyfill_cmd_name(&cmd_name, &bin_dirs);
    if *IS_STDOUT_TERMINAL {
        println!("{}", dimmed_text(&prompt));
    }
    let exit_code = run_command(&cmd_name, &cmd_args, Some(envs))
        .map_err(|err| anyhow!("Unable to run {cmd_name}, {err}"))?;
    if exit_code != 0 {
        bail!("Tool call exit with {exit_code}");
    }
    let mut output = None;
    if temp_file.exists() {
        let contents =
            fs::read_to_string(temp_file).context("Failed to retrieve tool call output")?;
        if !contents.is_empty() {
            output = Some(contents);
        }
    };
    Ok(output)
}

#[cfg(windows)]
fn polyfill_cmd_name<T: AsRef<Path>>(cmd_name: &str, bin_dir: &[T]) -> String {
    let cmd_name = cmd_name.to_string();
    if let Ok(exts) = std::env::var("PATHEXT") {
        for name in exts.split(';').map(|ext| format!("{cmd_name}{ext}")) {
            for dir in bin_dir {
                let path = dir.as_ref().join(&name);
                if path.exists() {
                    return name.to_string();
                }
            }
        }
    }
    cmd_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::RwLock;
    use std::sync::Arc;

    fn declaration(name: &str) -> FunctionDeclaration {
        serde_json::from_value(json!({
            "name": name,
            "description": "test function",
            "parameters": { "type": "object" }
        }))
        .unwrap()
    }

    fn config_with_functions(names: &[&str]) -> GlobalConfig {
        let config = Config {
            functions: Functions {
                declarations: names.iter().map(|name| declaration(name)).collect(),
            },
            ..Default::default()
        };
        Arc::new(RwLock::new(config))
    }

    fn assert_tool_error(result: &ToolResult, id: &str) {
        assert_eq!(result.call.id.as_deref(), Some(id));
        assert_eq!(
            result.output,
            json!({
                "error": {
                    "type": "tool_execution_error",
                    "message": "The tool call failed. Fix its arguments or choose another tool."
                }
            })
        );
    }

    #[test]
    fn mixed_tool_batch_preserves_order_ids_success_and_null_results() {
        let calls = vec![
            ToolCall::new("success".into(), json!({}), Some("call-1".into())),
            ToolCall::new("null".into(), json!({}), Some("call-2".into())),
            ToolCall::new("failure".into(), json!({}), Some("call-3".into())),
        ];

        let results = eval_tool_calls_with(calls, |call| match call.name.as_str() {
            "success" => Ok(json!({"ok": true})),
            "null" => Ok(Value::Null),
            _ => bail!("private execution details"),
        })
        .unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].call.id.as_deref(), Some("call-1"));
        assert_eq!(results[0].output, json!({"ok": true}));
        assert_eq!(results[1].call.id.as_deref(), Some("call-2"));
        assert_eq!(results[1].output, json!("DONE"));
        assert_tool_error(&results[2], "call-3");
        assert!(!results[2].output.to_string().contains("private"));
    }

    #[test]
    fn all_null_tool_batch_keeps_existing_empty_result_semantics() {
        let calls = vec![
            ToolCall::new("null".into(), json!({}), Some("call-1".into())),
            ToolCall::new("null".into(), json!({}), Some("call-2".into())),
        ];

        assert!(eval_tool_calls_with(calls, |_| Ok(Value::Null))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn preserving_results_keeps_one_output_per_null_call() {
        let calls = vec![
            ToolCall::new("null".into(), json!({}), Some("call-1".into())),
            ToolCall::new("null".into(), json!({}), Some("call-2".into())),
        ];

        let results = eval_tool_calls_with_options(calls, |_| Ok(Value::Null), false).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].call.id.as_deref(), Some("call-1"));
        assert_eq!(results[0].output, json!("DONE"));
        assert_eq!(results[1].call.id.as_deref(), Some("call-2"));
        assert_eq!(results[1].output, json!("DONE"));
    }

    #[test]
    fn unknown_tool_and_invalid_arguments_become_tool_results() {
        let unknown = eval_tool_calls(
            &config_with_functions(&[]),
            vec![ToolCall::new(
                "unknown-tool".into(),
                json!({"value": "private-unknown"}),
                Some("unknown-id".into()),
            )],
        )
        .unwrap();
        assert_tool_error(&unknown[0], "unknown-id");
        assert!(!unknown[0].output.to_string().contains("private-unknown"));

        let invalid = eval_tool_calls(
            &config_with_functions(&["invalid-arguments"]),
            vec![ToolCall::new(
                "invalid-arguments".into(),
                json!("not JSON: private-invalid"),
                Some("invalid-id".into()),
            )],
        )
        .unwrap();
        assert_tool_error(&invalid[0], "invalid-id");
        assert!(!invalid[0].output.to_string().contains("private-invalid"));
    }

    #[test]
    fn missing_executable_and_nonzero_exit_become_ordered_tool_results() {
        let missing = "aichat-test-command-that-does-not-exist-7d7da216";
        let nonzero = nonzero_command();
        let config = config_with_functions(&[missing, nonzero]);
        let results = eval_tool_calls(
            &config,
            vec![
                ToolCall::new(
                    missing.into(),
                    json!({"value": "private-missing"}),
                    Some("missing-id".into()),
                ),
                ToolCall::new(
                    nonzero.into(),
                    json!({"value": "private-nonzero"}),
                    Some("nonzero-id".into()),
                ),
            ],
        )
        .unwrap();

        assert_eq!(results.len(), 2);
        assert_tool_error(&results[0], "missing-id");
        assert_tool_error(&results[1], "nonzero-id");
        assert!(!results[0].output.to_string().contains("private-missing"));
        assert!(!results[1].output.to_string().contains("private-nonzero"));
    }

    #[cfg(not(windows))]
    fn nonzero_command() -> &'static str {
        "false"
    }

    #[cfg(windows)]
    fn nonzero_command() -> &'static str {
        "where.exe"
    }

    // --- Backlog #6a: tool safety mode parsing & classification ---

    #[test]
    fn mode_readonly_parses_and_classifies() {
        let decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "fs_cat",
            "description": "read a file",
            "parameters": {"type": "object"},
            "mode": "readonly"
        }))
        .unwrap();
        assert_eq!(decl.mode, Some(ToolMode::Readonly));
        assert_eq!(decl.safety_class(), SafetyClass::Readonly);
        assert!(decl.safety_class().allowed_under_readonly_mask());
    }

    #[test]
    fn mode_mutating_parses_and_classifies() {
        let decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "fs_write",
            "description": "write a file",
            "parameters": {"type": "object"},
            "mode": "mutating"
        }))
        .unwrap();
        assert_eq!(decl.mode, Some(ToolMode::Mutating));
        assert_eq!(decl.safety_class(), SafetyClass::Mutating);
        assert!(
            !decl.safety_class().allowed_under_readonly_mask(),
            "mutating tools must be denied under a readonly mask"
        );
    }

    #[test]
    fn absent_mode_is_unclassified_and_denied_under_mask() {
        // FR-6a.2: a tool with no declared mode is *unclassified* — the most
        // conservative disposition (reserved to humans), NOT an implicit mutating.
        let decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "some_tool",
            "description": "no mode declared",
            "parameters": {"type": "object"}
        }))
        .unwrap();
        assert_eq!(decl.mode, None);
        assert_eq!(decl.safety_class(), SafetyClass::Unclassified);
        assert!(
            !decl.safety_class().allowed_under_readonly_mask(),
            "unclassified tools must be denied under a readonly mask"
        );
    }

    #[test]
    fn mode_is_not_serialized_to_the_llm() {
        // Governance metadata must never leak into the schema sent to the model
        // (matches the skip_serializing treatment of `agent` and `output`).
        let decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "fs_write",
            "description": "write a file",
            "parameters": {"type": "object"},
            "mode": "mutating"
        }))
        .unwrap();
        let serialized = serde_json::to_value(&decl).unwrap();
        assert!(
            serialized.get("mode").is_none(),
            "mode must be skipped during serialization, got {serialized}"
        );
    }

    #[test]
    fn unclassified_is_stricter_than_mutating_but_distinct() {
        // Both are denied under a readonly mask, but they are distinguishable so
        // later increments can apply the stricter reserved-to-humans policy only
        // to the unclassified case.
        assert_ne!(SafetyClass::Unclassified, SafetyClass::Mutating);
        assert!(!SafetyClass::Unclassified.allowed_under_readonly_mask());
        assert!(!SafetyClass::Mutating.allowed_under_readonly_mask());
        assert!(SafetyClass::Readonly.allowed_under_readonly_mask());
    }

    // --- Backlog #6b: blast-radius tiers, ordering, static_tier resolution ---

    #[test]
    fn blast_radius_is_totally_ordered_by_increasing_danger() {
        assert!(BlastRadius::Safe < BlastRadius::Reversible);
        assert!(BlastRadius::Reversible < BlastRadius::Disruptive);
        assert!(BlastRadius::Disruptive < BlastRadius::Destructive);
        assert!(BlastRadius::Destructive < BlastRadius::Catastrophic);
        // max() picks the more dangerous tier — the basis of the stricter-only clamp.
        assert_eq!(
            std::cmp::max(BlastRadius::Safe, BlastRadius::Destructive),
            BlastRadius::Destructive
        );
    }

    #[test]
    fn blast_radius_parses_lowercase() {
        for (s, want) in [
            ("safe", BlastRadius::Safe),
            ("reversible", BlastRadius::Reversible),
            ("disruptive", BlastRadius::Disruptive),
            ("destructive", BlastRadius::Destructive),
            ("catastrophic", BlastRadius::Catastrophic),
        ] {
            let decl: FunctionDeclaration = serde_json::from_value(json!({
                "name": "t",
                "description": "d",
                "parameters": {"type": "object"},
                "risk": s
            }))
            .unwrap();
            assert_eq!(decl.risk, Some(want), "risk {s} should parse to {want:?}");
        }
    }

    #[test]
    fn risk_and_reversibility_not_serialized_to_the_llm() {
        let decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "fs_delete",
            "description": "delete a file",
            "parameters": {"type": "object"},
            "risk": "destructive",
            "reversible": false
        }))
        .unwrap();
        let serialized = serde_json::to_value(&decl).unwrap();
        assert!(serialized.get("risk").is_none(), "risk must be skipped");
        assert!(
            serialized.get("reversible").is_none(),
            "reversible must be skipped"
        );
    }

    #[test]
    fn static_tier_prefers_explicit_risk() {
        let decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "t",
            "description": "d",
            "parameters": {"type": "object"},
            "risk": "catastrophic",
            "mode": "readonly"
        }))
        .unwrap();
        // Explicit risk wins even over a (contradictory) mode.
        assert_eq!(
            decl.static_tier(),
            StaticTier::Tier(BlastRadius::Catastrophic)
        );
    }

    #[test]
    fn static_tier_maps_legacy_mode_when_no_risk() {
        // readonly → Safe
        let ro: FunctionDeclaration = serde_json::from_value(json!({
            "name": "fs_cat", "description": "d", "parameters": {"type": "object"}, "mode": "readonly"
        }))
        .unwrap();
        assert_eq!(ro.static_tier(), StaticTier::Tier(BlastRadius::Safe));

        // mutating → Disruptive (conservative floor for declared-mutating)
        let mu: FunctionDeclaration = serde_json::from_value(json!({
            "name": "fs_write", "description": "d", "parameters": {"type": "object"}, "mode": "mutating"
        }))
        .unwrap();
        assert_eq!(mu.static_tier(), StaticTier::Tier(BlastRadius::Disruptive));
    }

    #[test]
    fn static_tier_unclassified_when_nothing_declared() {
        let decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "mystery", "description": "d", "parameters": {"type": "object"}
        }))
        .unwrap();
        assert_eq!(decl.static_tier(), StaticTier::Unclassified);
    }

    #[test]
    fn delegated_permissions_parsing_nested_and_flat() {
        let nested = json!({
            "task": "do work",
            "permissions": {
                "mask": "mutating",
                "ceiling": "reversible"
            }
        });
        let p_nested = DelegatedPermissions::parse_from_value(&nested).unwrap();
        assert_eq!(p_nested.mask.as_deref(), Some("mutating"));
        assert_eq!(p_nested.ceiling.as_deref(), Some("reversible"));

        let flat = json!({
            "task": "do work",
            "permissions_mask": "mutating",
            "permissions_ceiling": "disruptive"
        });
        let p_flat = DelegatedPermissions::parse_from_value(&flat).unwrap();
        assert_eq!(p_flat.mask.as_deref(), Some("mutating"));
        assert_eq!(p_flat.ceiling.as_deref(), Some("disruptive"));

        let str_args = json!("{\"permissions_mask\": \"readonly\", \"permissions_ceiling\": \"safe\"}");
        let p_str = DelegatedPermissions::parse_from_value(&str_args).unwrap();
        assert_eq!(p_str.mask.as_deref(), Some("readonly"));
        assert_eq!(p_str.ceiling.as_deref(), Some("safe"));
    }

    #[test]
    fn delegated_permissions_validation_and_clamping() {
        use crate::safety::AuthorityCeiling;

        // Valid sub-agent provisioning within parent limits
        let valid = DelegatedPermissions {
            mask: Some("mutating".into()),
            ceiling: Some("reversible".into()),
        };
        let (mask, ceiling) = valid
            .validate_against_parent(false, AuthorityCeiling::UpTo(BlastRadius::Disruptive))
            .unwrap();
        assert_eq!(mask, "mutating");
        assert_eq!(ceiling, AuthorityCeiling::UpTo(BlastRadius::Reversible));

        // Readonly parent cannot provision mutating mask
        let mutating_req = DelegatedPermissions {
            mask: Some("mutating".into()),
            ceiling: Some("safe".into()),
        };
        assert!(mutating_req
            .validate_against_parent(true, AuthorityCeiling::UpTo(BlastRadius::Disruptive))
            .is_err());

        // Sub-agent cannot exceed parent ceiling
        let over_ceiling = DelegatedPermissions {
            mask: Some("mutating".into()),
            ceiling: Some("destructive".into()),
        };
        assert!(over_ceiling
            .validate_against_parent(false, AuthorityCeiling::UpTo(BlastRadius::Reversible))
            .is_err());

        // Catastrophic ceiling is reserved to humans
        let cat_req = DelegatedPermissions {
            mask: Some("mutating".into()),
            ceiling: Some("catastrophic".into()),
        };
        assert!(cat_req
            .validate_against_parent(false, AuthorityCeiling::UpTo(BlastRadius::Catastrophic))
            .is_err());

        // Malformed/unknown values clamp closed to safe floor
        let malformed = DelegatedPermissions {
            mask: Some("unlimited_power".into()),
            ceiling: Some("infinite".into()),
        };
        let (clamp_m, clamp_c) = malformed
            .validate_against_parent(false, AuthorityCeiling::UpTo(BlastRadius::Disruptive))
            .unwrap();
        assert_eq!(clamp_m, "readonly");
        assert_eq!(clamp_c, AuthorityCeiling::MINIMAL);
    }

    #[test]
    fn enrich_agent_permissions_schema_adds_properties() {
        let mut decl = FunctionDeclaration {
            name: "coder".into(),
            description: "Agent".into(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"}
                }
            })).unwrap(),
            agent: true,
            output: None,
            mode: None,
            risk: None,
            reversible: None,
            reversible_via: None,
            nano: None,
        };
        decl.enrich_agent_permissions_schema();
        let props = decl.parameters.properties.unwrap();
        assert!(props.contains_key("permissions"));
        assert!(props.contains_key("permissions_mask"));
        assert!(props.contains_key("permissions_ceiling"));
    }

    #[test]
    fn test_function_declaration_nano_property() {
        let decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "web_search",
            "description": "search the web",
            "parameters": { "type": "object" },
            "nano": true
        }))
        .unwrap();
        assert!(decl.is_nano());

        let regular_decl: FunctionDeclaration = serde_json::from_value(json!({
            "name": "fs_cat",
            "description": "cat a file",
            "parameters": { "type": "object" }
        }))
        .unwrap();
        assert!(!regular_decl.is_nano());
    }
}
