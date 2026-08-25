# Tool Output Routing — Requirements

## Summary

Allow tool declarations to specify where their output goes after execution, instead of always returning it to the LLM's conversation context. Three routing destinations: context (default), file (write to disk, return confirmation), and pipe (pass output directly to another tool). Additionally, implement automatic large-result capping (FR-7 from the agent loop spec) as the baseline behavior that routing builds upon.

## Context

- Current state: every tool result — regardless of size or purpose — goes back into the conversation as a `tool_result` message. A `fetch_url` that returns 200KB of HTML burns tokens for the full content. A `generate_report` tool produces a file as a side effect AND stuffs its content into context. A multi-step pipeline (fetch → transform → summarize) round-trips through the LLM for every intermediate step.
- The agent loop (backlog #3, complete) provides the execution layer: `eval_tool_calls_parallel` dispatches tools and collects `ToolResult` values. Routing integrates at the point where results are collected — between "tool executed" and "result merged into conversation."
- The `FunctionDeclaration` struct already has extensible serde fields (`#[serde(skip_serializing, default)]`). Adding an optional `output` field is straightforward.

## Design Philosophy

1. **Declarative and optional.** Routing is declared per-tool in `functions.json`. Tools without routing declarations behave exactly as before (output → context). Zero config change required for existing setups.
2. **Transparent to the LLM.** The model never sees the routing machinery. It calls a tool and gets a result back — either the full output (context), a confirmation (file), or the final piped result (pipe). The model doesn't need to know about routing.
3. **Composable with existing features.** Routing works with parallel execution, sub-agents, MCP tools, and the planning tool. A piped chain counts as one tool execution for progress reporting and budget purposes.
4. **Large-result capping as baseline.** Even without explicit routing declarations, results exceeding `tool_output_limit` are automatically capped (written to temp file, preview returned). Explicit routing overrides this behavior for declared tools.

## Functional Requirements

### FR-1: Output Routing Declaration

- FR-1.1: `FunctionDeclaration` MUST support an optional `output` field specifying routing behavior.
- FR-1.2: The `output` field MUST be deserialized from `functions.json` but MUST NOT be serialized when sending tool definitions to the LLM (the model doesn't need to see it).
- FR-1.3: Three destination types MUST be supported:
  - `context` — default behavior, result goes into conversation (explicit declaration optional)
  - `file` — result written to a path, model receives confirmation
  - `pipe` — result passed as input to another tool, model receives the final output
- FR-1.4: When `output` is absent or null, the behavior MUST be `context` (backward compatible).

### FR-2: File Destination

- FR-2.1: When a tool's output is routed to `file`, the full output MUST be written to the specified path.
- FR-2.2: The path MUST support template variables:
  - `{{name}}` — the tool name
  - `{{id}}` — the tool call ID (if present)
  - `{{timestamp}}` — Unix timestamp
  - `{{ext}}` — inferred extension from content type (default: `txt`)
- FR-2.3: The model MUST receive a confirmation result instead of the full output:
  ```json
  {"written_to": "/tmp/report.md", "size_bytes": 24576, "hint": "847 lines"}
  ```
- FR-2.4: Parent directories MUST be created if they don't exist.
- FR-2.5: If the file write fails, the full output MUST fall back to `context` destination (don't lose the result).

### FR-3: Pipe Destination

- FR-3.1: When a tool's output is routed to `pipe`, the output MUST be passed as the input argument to the target tool.
- FR-3.2: The target tool MUST be specified by name in the declaration: `"output": {"destination": "pipe", "target": "summarize_data"}`.
- FR-3.3: The piped tool MUST be invoked with the source tool's output as a JSON argument: `{"input": "<source_output>"}` (or the tool's first required parameter if it has one).
- FR-3.4: The piped tool's result is what the model sees — the intermediate result is never exposed to the LLM.
- FR-3.5: Pipe chains MUST be acyclic — if tool A pipes to tool B which pipes to tool C, that's fine. If tool A pipes to tool A (or any cycle), it MUST be detected and rejected with an error at dispatch time.
- FR-3.6: A piped chain MUST count as a single tool execution for:
  - Turn budget (one turn consumed)
  - Progress reporting (one ToolStart/ToolComplete pair for the chain)
  - Concurrency (one semaphore permit for the whole chain)
- FR-3.7: If any tool in the pipe chain fails, the error MUST be returned to the model (no silent failures).
- FR-3.8: The target tool MUST exist in the current tool set. If it doesn't, return an error result.

### FR-4: Automatic Large-Result Capping (FR-7 implementation)

- FR-4.1: When a tool result (routed to `context`) exceeds `tool_output_limit` (default 16 KB), it MUST be automatically capped.
- FR-4.2: Capping behavior: write full output to a temp file, return a bounded preview to the model.
- FR-4.3: The preview MUST contain:
  - The first N characters up to the threshold
  - Path to the full output file
  - Total byte count
  - A shape hint (line count for text, top-level keys for JSON objects)
- FR-4.4: Format: `{"preview": "<first 16KB>", "full_output_path": "/tmp/aichat-tool-<id>.out", "total_bytes": 524288, "hint": "2847 lines"}`
- FR-4.5: Temp files MUST be cleaned up on process exit.
- FR-4.6: Explicit `file` routing MUST bypass capping (the output is already being written to a file).
- FR-4.7: `pipe` destination outputs are NOT capped at the intermediate stage (only the final result reaching the model is subject to capping).
- FR-4.8: Capping MUST NOT apply to error results or `_plan` results.

### FR-5: Declaration Format

- FR-5.1: The `output` field in `functions.json`:
  ```json
  {
    "name": "tool_name",
    "description": "...",
    "parameters": { ... },
    "output": {
      "destination": "file",
      "path": "/tmp/{{name}}-{{timestamp}}.md"
    }
  }
  ```
  ```json
  {
    "name": "tool_name",
    "description": "...",
    "parameters": { ... },
    "output": {
      "destination": "pipe",
      "target": "other_tool"
    }
  }
  ```
- FR-5.2: The `output` field MUST be optional. Omitting it = `context` destination.
- FR-5.3: Invalid routing declarations (unknown destination, missing target for pipe) MUST produce a warning at tool load time and fall back to `context`.

## Configuration

No new top-level config section. The routing is per-tool via `functions.json`. The capping threshold is already in `agent_loop.tool_output_limit` (from backlog #3 spec).

## Non-Functional Requirements

- NFR-1: Routing adds less than 1ms overhead per tool result (a path template expansion + conditional file write).
- NFR-2: Pipe chains must not introduce unbounded memory usage — intermediate results are passed by value (not buffered indefinitely).
- NFR-3: All existing tests must continue to pass. New tests must cover: file routing, pipe routing, capping, cycle detection, template expansion.
- NFR-4: Tools without `output` declarations must behave identically to before (zero regression).

## Out of Scope

- Dynamic routing specified by the LLM at call time (future consideration — `_output` argument pattern)
- Streaming between piped tools (batch only: source finishes, then target starts)
- Routing MCP tool results differently from shell-exec tools (same routing applies uniformly)
- Network destinations (HTTP POST, webhook) — file and pipe cover the local use cases
