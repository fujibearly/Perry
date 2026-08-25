# Tool Output Routing — Design

## Module Structure

```
src/
├── agent_loop.rs              # Modified: routing applied after tool execution, capping logic
├── function.rs                # Modified: OutputRouting struct on FunctionDeclaration
└── config/
    └── mod.rs                 # No changes (tool_output_limit already in AgentLoopConfig)
```

No new files. The routing logic lives in `agent_loop.rs` alongside the parallel dispatch — it's a post-processing step on `ToolResult` values before they're merged into the conversation.

## Data Structures

### Output Routing (in `src/function.rs`)

```rust
/// Routing declaration for a tool's output.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OutputRouting {
    /// Destination: "context" (default), "file", or "pipe".
    pub destination: OutputDestination,
    /// File path template (for "file" destination).
    pub path: Option<String>,
    /// Target tool name (for "pipe" destination).
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputDestination {
    #[default]
    Context,
    File,
    Pipe,
}
```

Added to `FunctionDeclaration`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
    #[serde(skip_serializing, default)]
    pub agent: bool,
    /// Output routing: where this tool's result goes after execution.
    #[serde(skip_serializing, default)]
    pub output: Option<OutputRouting>,
}
```

`skip_serializing` ensures the LLM never sees the `output` field in tool definitions — it's a harness-level concern, not part of the tool schema the model operates on.

## Integration Point

The routing logic applies inside `eval_tool_calls_parallel`, after a tool completes but before its `ToolResult` is returned. The flow:

```
eval_single_tool() returns Value
  │
  ├─ Check declaration's output routing
  │
  ├─ OutputDestination::Context (or no declaration)
  │     └─ Apply capping if exceeds tool_output_limit
  │          ├─ Under limit → return as-is
  │          └─ Over limit → write to temp file, return preview
  │
  ├─ OutputDestination::File
  │     └─ Write full output to path (expand templates)
  │        Return confirmation: {"written_to": path, "size_bytes": N, "hint": "..."}
  │
  └─ OutputDestination::Pipe
        └─ Invoke target tool with output as input
           Apply routing recursively to target's result
           Return final result to caller
```

### Modified `eval_tool_calls_parallel` flow

```rust
// After tool execution, before constructing ToolResult:
let output = match result {
    Ok(value) => {
        // Apply routing
        let routed = apply_output_routing(
            &config, &call.name, value, tool_output_limit
        ).await?;
        routed
    }
    Err(_) => { /* error result — no routing applied */ }
};
```

## Output Routing Implementation

### `apply_output_routing()`

```rust
async fn apply_output_routing(
    config: &GlobalConfig,
    tool_name: &str,
    output: Value,
    tool_output_limit: usize,
) -> Result<Value> {
    // Look up the tool's routing declaration
    let routing = {
        let config_read = config.read();
        get_tool_routing(&config_read, tool_name)
    };

    match routing.map(|r| &r.destination) {
        Some(OutputDestination::File) => {
            route_to_file(output, routing.unwrap()).await
        }
        Some(OutputDestination::Pipe) => {
            let target = routing.unwrap().target.as_ref().unwrap();
            route_to_pipe(config, output, target).await
        }
        _ => {
            // Context destination (default) — apply capping
            apply_capping(output, tool_name, tool_output_limit)
        }
    }
}
```

### File routing

```rust
async fn route_to_file(output: Value, routing: &OutputRouting) -> Result<Value> {
    let content = value_to_string(&output);
    let path = expand_path_template(routing.path.as_ref().unwrap(), &context);

    // Create parent dirs
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write
    match std::fs::write(&path, &content) {
        Ok(()) => {
            let lines = content.lines().count();
            Ok(json!({
                "written_to": path,
                "size_bytes": content.len(),
                "hint": format!("{lines} lines")
            }))
        }
        Err(e) => {
            // Fall back to context on write failure
            warn!("File routing failed for {}: {e}. Falling back to context.", path);
            Ok(output)
        }
    }
}
```

### Pipe routing

```rust
async fn route_to_pipe(
    config: &GlobalConfig,
    output: Value,
    target_tool: &str,
) -> Result<Value> {
    // Build a synthetic ToolCall for the target
    let pipe_call = ToolCall::new(
        target_tool.to_string(),
        json!({"input": value_to_string(&output)}),
        None,
    );

    // Execute the target tool
    let result = eval_single_tool(config, &pipe_call).await?;

    // Recursively apply routing to the target's result
    let tool_output_limit = config.read().agent_loop.tool_output_limit;
    apply_output_routing(config, target_tool, result, tool_output_limit).await
}
```

### Capping (auto, for context destination)

```rust
fn apply_capping(output: Value, tool_name: &str, limit: usize) -> Result<Value> {
    let content = value_to_string(&output);
    if content.len() <= limit {
        return Ok(output);
    }

    // Write full output to temp file
    let path = format!("/tmp/aichat-tool-{}-{}.out", tool_name, std::process::id());
    std::fs::write(&path, &content)?;

    // Build preview
    let preview: String = content.chars().take(limit).collect();
    let lines = content.lines().count();
    let hint = if output.is_object() {
        let keys: Vec<&str> = output.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        format!("JSON object with keys: {}", keys.join(", "))
    } else {
        format!("{lines} lines")
    };

    Ok(json!({
        "preview": preview,
        "full_output_path": path,
        "total_bytes": content.len(),
        "hint": hint
    }))
}
```

### Cycle detection for pipes

Before invoking a pipe chain, build the chain of target names and check for duplicates:

```rust
fn detect_pipe_cycle(config: &GlobalConfig, start_tool: &str) -> Result<()> {
    let mut visited = HashSet::new();
    let mut current = start_tool.to_string();
    visited.insert(current.clone());

    loop {
        let routing = get_tool_routing(&config.read(), &current);
        match routing {
            Some(r) if r.destination == OutputDestination::Pipe => {
                let target = r.target.as_ref().unwrap().clone();
                if !visited.insert(target.clone()) {
                    bail!("Pipe cycle detected: {} → ... → {}", start_tool, target);
                }
                current = target;
            }
            _ => break,
        }
    }
    Ok(())
}
```

### Path template expansion

```rust
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
```

## Helper: `value_to_string()`

Converts a `serde_json::Value` to a string for file writing or size checking:

```rust
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => serde_json::to_string_pretty(value).unwrap_or_default(),
    }
}
```

## Temp File Cleanup

Capped output files (`/tmp/aichat-tool-*`) are cleaned up on process exit alongside the status file. The existing `cleanup_status_file()` path is extended:

```rust
pub fn cleanup_temp_files() {
    cleanup_status_file();
    // Clean tool output temp files for this process
    let pid = std::process::id();
    let pattern = format!("/tmp/aichat-tool-*-{pid}.out");
    if let Ok(entries) = glob::glob(&pattern) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry);
        }
    }
}
```

Alternatively (simpler, no glob dependency): track temp file paths in a `Vec<PathBuf>` on the progress struct and clean them in the Drop impl.

## Backward Compatibility

- Tools without `output` field: behavior unchanged (result → context, with capping if > 16KB)
- The `output` field uses `#[serde(skip_serializing, default)]` — never sent to LLM, doesn't affect existing `functions.json` files that lack it
- Capping is new behavior for large results, but it's an improvement (prevents context blowout) — the full content is still accessible via the temp file path

## Testing Strategy

1. **File routing:** tool output → written to path, model gets confirmation JSON
2. **Pipe routing:** tool A → piped to tool B, model gets tool B's result
3. **Capping:** result > 16KB → preview + temp file path returned
4. **Capping bypass:** file-routed tools are not capped
5. **Pipe cycle detection:** A→B→A produces error
6. **Template expansion:** `{{name}}`, `{{timestamp}}`, etc. resolve correctly
7. **Fallback on write error:** invalid path → falls back to context (no crash)
8. **No routing:** tools without `output` → unchanged behavior
