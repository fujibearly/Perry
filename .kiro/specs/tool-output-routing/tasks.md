# Tool Output Routing — Implementation Tasks

## Task 1: Add OutputRouting to FunctionDeclaration

**Files:** `src/function.rs`

1. Add `OutputRouting` struct and `OutputDestination` enum:
   ```rust
   #[derive(Debug, Clone, Default, Deserialize)]
   #[serde(default)]
   pub struct OutputRouting {
       pub destination: OutputDestination,
       pub path: Option<String>,
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
2. Add to `FunctionDeclaration`:
   ```rust
   #[serde(skip_serializing, default)]
   pub output: Option<OutputRouting>,
   ```
3. Verify: `cargo build`. Existing `functions.json` files without `output` field still load correctly (serde default).

---

## Task 2: Implement large-result capping

**Files:** `src/agent_loop.rs`

1. Add `fn apply_capping(output: Value, tool_name: &str, limit: usize) -> Value`:
   - If `value_to_string(&output).len() <= limit` → return output unchanged
   - Otherwise: write full content to `/tmp/aichat-tool-<name>-<pid>.out`
   - Return preview JSON: `{"preview": "<first N chars>", "full_output_path": path, "total_bytes": N, "hint": "..."}`
   - Hint: line count for strings, top-level keys for JSON objects
2. Add `fn value_to_string(value: &Value) -> String`:
   - String values → as-is
   - Null → empty string
   - Other → `serde_json::to_string_pretty`
3. Integrate into `eval_tool_calls_parallel`: after `eval_single_tool` returns successfully, apply capping before constructing `ToolResult`.
4. Track temp file paths for cleanup (store in a `static` or on the progress struct).
5. Extend `cleanup_status_file()` → `cleanup_temp_files()` to also remove capped output files.
6. Verify: Test with a tool that returns > 16KB → preview returned, full content in temp file.

---

## Task 3: Implement file routing

**Files:** `src/agent_loop.rs`

1. Add `fn expand_path_template(template: &str, tool_name: &str, call_id: Option<&str>) -> String`:
   - Replace `{{name}}`, `{{id}}`, `{{timestamp}}`, `{{ext}}`
2. Add `async fn route_to_file(output: Value, tool_name: &str, routing: &OutputRouting) -> Value`:
   - Expand path template
   - Create parent directories
   - Write content via `value_to_string()`
   - Return confirmation: `{"written_to": path, "size_bytes": N, "hint": "N lines"}`
   - On write failure: log warning, fall back to returning the original output
3. Add routing lookup: `fn get_tool_routing(config: &Config, tool_name: &str) -> Option<&OutputRouting>`:
   - Check agent functions first (if in agent context), then global functions
4. Integrate into the dispatch path: after `eval_single_tool`, check routing → if `File`, call `route_to_file` instead of returning raw output.
5. File-routed results bypass capping (they're already written to a file).
6. Verify: Declare a tool with `"output": {"destination": "file", "path": "/tmp/{{name}}.md"}`, invoke it, confirm file written and model gets confirmation.

---

## Task 4: Implement pipe routing

**Files:** `src/agent_loop.rs`

1. Add `fn detect_pipe_cycle(config: &GlobalConfig, start_tool: &str) -> Result<()>`:
   - Walk the pipe chain, track visited tool names, bail on cycle
2. Add `async fn route_to_pipe(config: &GlobalConfig, output: Value, target_tool: &str) -> Result<Value>`:
   - Build synthetic `ToolCall` with the source output as `{"input": content}`
   - Call `eval_single_tool` for the target
   - Recursively apply `apply_output_routing` to the target's result (handles chained pipes)
3. Integrate into the dispatch path: after `eval_single_tool`, check routing → if `Pipe`:
   - Run cycle detection first
   - Call `route_to_pipe`
   - The final result (after chain) is what goes into `ToolResult`
4. The entire pipe chain runs under one semaphore permit and one ToolStart/ToolComplete pair.
5. Verify: Declare tool A with `"output": {"destination": "pipe", "target": "B"}`, invoke A, confirm model sees B's result. Test cycle detection with A→B→A.

---

## Task 5: Wire routing into the dispatch loop

**Files:** `src/agent_loop.rs`

1. Add `async fn apply_output_routing(config: &GlobalConfig, tool_name: &str, output: Value, limit: usize) -> Value`:
   - Look up routing declaration
   - Match on destination: File → `route_to_file`, Pipe → `route_to_pipe`, Context → `apply_capping`
2. Call `apply_output_routing` in the parallel dispatch closure (inside `eval_tool_calls_parallel`), after `eval_single_tool` succeeds and before constructing `ToolResult`.
3. Ensure error results are never routed (only successful outputs).
4. Ensure `_plan` results are never routed or capped.
5. Verify: End-to-end test with all three routing types in one turn.

---

## Task 6: Tests

**Files:** `src/agent_loop.rs` (test module)

1. **Capping test:** Value > 16KB → preview + temp file path returned, file exists on disk.
2. **Capping bypass for small results:** Value < 16KB → returned unchanged.
3. **File routing test:** Declared tool with file destination → output written, confirmation returned.
4. **File routing fallback:** Invalid path → original output returned (no crash).
5. **Pipe routing test:** A pipes to B → model gets B's result.
6. **Pipe cycle detection:** A→B→A → error.
7. **Template expansion test:** `{{name}}`, `{{timestamp}}`, `{{id}}` all resolve.
8. **No routing (backward compat):** Tool without `output` field → result goes to context unchanged (under limit).
9. **Capping doesn't apply to errors:** Error result → returned as-is regardless of size.
10. Verify: `cargo test` — all pass.

---

## Execution Order

```
T1 (data structures) → T2 (capping) → T3 (file routing) → T4 (pipe routing) → T5 (wiring) → T6 (tests)
```

Linear dependency chain — each task builds on the previous.

### Estimated scope

| Task | Lines (approx) | Risk |
|------|----------------|------|
| T1 | ~20 | Low — additive struct, serde default |
| T2 | ~60 | Low — file I/O with fallback |
| T3 | ~50 | Low — template expansion + file write |
| T4 | ~60 | Medium — recursive routing, cycle detection |
| T5 | ~30 | Low — wiring existing functions together |
| T6 | ~80 | Low — tests |
| **Total** | **~300** | |
