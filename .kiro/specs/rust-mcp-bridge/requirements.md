# Rust MCP Bridge — Requirements

## Summary

Replace the Node.js MCP bridge with a Rust implementation embedded in aichat. MCP-sourced tools appear identical to shell-exec tools from the perspective of the rest of the codebase. No new dispatch abstraction, no trait, no special path — MCP tools are just `FunctionDeclaration` entries that happen to route through an internal async stdio connection instead of a subprocess + HTTP + curl chain.

## Context

- Current state: MCP tools work via an external Node.js bridge (`llm-functions/mcp/bridge/index.js`) that spawns MCP servers, exposes them as HTTP on localhost:8808, and shell shims curl the bridge when aichat invokes a tool.
- Problem: Adds ~200ms latency per call, requires Node.js runtime, fragile subprocess chain, hard to debug.
- Goal: aichat speaks MCP directly over stdio (spawn server binary, JSON-RPC 2.0) with zero external runtime dependencies.
- Philosophy: The rest of the codebase doesn't know MCP exists. `eval_tool_calls` sees a `ToolCall`, dispatches it, gets a result. Whether that call went to a shell script or an MCP server is an implementation detail hidden behind the existing call evaluation path.

## Design Philosophy (Option C)

This implementation follows sigoden's upstream philosophy:

1. **Tools look like tools.** MCP-sourced tools produce `FunctionDeclaration` entries indistinguishable from shell-exec tools. The LLM, the agent system, `use_tools` filtering, `--info` — all see the same thing.
2. **Minimal Rust surface.** The MCP module is self-contained. It exports two things: a way to discover tools (returns `Vec<FunctionDeclaration>`) and a way to call them (takes name + args, returns string). Nothing else leaks.
3. **No new abstractions.** No `ToolDispatch` trait, no enum of backends, no registry pattern. The integration point is inside `ToolCall::eval()` — if the tool name matches an MCP-sourced tool, route to the MCP module; otherwise, fall through to shell-exec as today.
4. **Composable with existing ecosystem.** The existing Node.js MCP server (`mcp/server/index.js`) continues to work unchanged for exposing aichat's tools to external clients. The native client replaces only the *consumption* path.
5. **Async-ready but not async-forced.** The MCP module is internally async (tokio). The call interface can be `block_on` from the synchronous `eval_tool_calls` today, and naturally becomes `await` when backlog #3 makes `eval_tool_calls` async later. No refactor needed at that point.

## Functional Requirements

### FR-1: MCP Server Lifecycle Management

- FR-1.1: aichat MUST spawn MCP server processes as child subprocesses, communicating via stdin/stdout (stdio transport).
- FR-1.2: aichat MUST send `initialize` request on first use and wait for the server's response before issuing tool calls.
- FR-1.3: aichat MUST send `notifications/initialized` after receiving the server's `initialize` response.
- FR-1.4: Server processes MUST be kept alive for the duration of the aichat session (REPL) or single command invocation (CLI one-shot).
- FR-1.5: On aichat exit (normal or abort signal), all spawned MCP server processes MUST be terminated gracefully (SIGTERM, then SIGKILL after 5s timeout).
- FR-1.6: Servers MUST be spawned lazily — only when a tool from that server is first invoked, not at aichat startup.
- FR-1.7: If a server process crashes mid-session, aichat MUST report the error for the current tool call and NOT attempt automatic restart.

### FR-2: Tool Discovery

- FR-2.1: After server initialization, aichat MUST send `tools/list` and receive the server's tool declarations.
- FR-2.2: MCP tool schemas (name, description, inputSchema) MUST be converted to aichat's existing `FunctionDeclaration` format.
- FR-2.3: Tool names MUST be namespaced to avoid collisions: `<server_name>__<tool_name>` (double underscore separator).
- FR-2.4: MCP tools MUST be merged into the same tool list as shell-exec tools. The LLM and agent system see one flat list.
- FR-2.5: Tool discovery MUST use a cached manifest. On first run (or `--sync-mcp`), aichat spawns each server, fetches `tools/list`, writes declarations to `<config-dir>/mcp-cache/<server-name>.json`, then shuts down the server. On subsequent runs, declarations are loaded from cache without spawning any server.
- FR-2.6: Cache MUST be invalidated automatically when the server's config entry changes (hash of `command` + `args` + `env` stored in cache file).
- FR-2.7: `--sync-mcp` CLI flag MUST force re-discovery of all configured MCP servers, updating the cache.
- FR-2.8: If cache is missing for a server and no `--sync-mcp` was given, aichat MUST perform discovery inline (spawn, fetch, cache, then proceed). First invocation is slower; subsequent ones are instant.

### FR-3: Tool Invocation

- FR-3.1: When `ToolCall::eval()` encounters a tool name matching an MCP-sourced tool, it MUST route to the MCP module instead of shell-exec.
- FR-3.2: The MCP module MUST send a `tools/call` JSON-RPC request with the arguments object and return the result as a `serde_json::Value`.
- FR-3.3: Text content blocks from the MCP response MUST be concatenated and returned as a string value.
- FR-3.4: Image content blocks MUST be returned as base64 data URIs when the model supports vision, otherwise discarded with a logged warning.
- FR-3.5: If the MCP response indicates `isError: true`, the result MUST follow the existing error pattern: `{"error": {"type": "tool_execution_error", "message": "..."}}`.
- FR-3.6: Tool calls MUST respect a per-call timeout (configurable, default 60s). On timeout, return an error result.
- FR-3.7: The call interface MUST return the same `Result<Value>` that `ToolCall::eval()` already returns. No new types at the boundary.

### FR-4: Configuration

- FR-4.1: MCP servers MUST be configurable in `config.yaml` under an `mcp_servers` key:
  ```yaml
  mcp_servers:
    - name: filesystem
      command: mcp-server-filesystem
      args: ["/home/user/projects"]
      env:
        SOME_VAR: value
    - name: git
      command: mcp-server-git
      args: [--repository, .]
      disabled: false
  ```
- FR-4.2: Each server entry MUST support: `name` (required, unique identifier), `command` (required, binary to spawn), `args` (optional, string list), `env` (optional, string map), `disabled` (optional, boolean, default false).
- FR-4.3: Agent-level MCP servers MUST be configurable in agent config (`agents/<name>/config.yaml`) using the same schema, merging with (not replacing) global servers.
- FR-4.4: Environment variables in `command`, `args`, and `env` values MUST be expanded (`$HOME`, `${VAR}`).
- FR-4.5: Duplicate server names (global vs agent) — agent-level wins.

### FR-5: Integration with Existing Tool System

- FR-5.1: MCP tools MUST participate in `use_tools` filtering. Server name acts as the tool group name (e.g., `use_tools: "filesystem,git"` includes tools from those MCP servers).
- FR-5.2: MCP tools MUST work in both classic agent loop and multi-agent loop without any special handling in those loops.
- FR-5.3: `--info` MUST show configured MCP servers, their status (configured/running/error), and tool count.
- FR-5.4: The `functions` role (`%functions%`) MUST include MCP tools when `use_tools: all`.

### FR-6: Compatibility with Existing MCP Server

- FR-6.1: The existing `mcp/server/index.js` (which exposes aichat's shell-exec tools as an MCP server) MUST continue to work unchanged.
- FR-6.2: MCP-sourced tools (consumed via native client) are NOT re-exported through the MCP server. They are internal to aichat's session.

## Non-Functional Requirements

### NFR-1: Performance

- NFR-1.1: Tool invocation overhead (aichat → JSON-RPC → server → response parse) MUST be under 10ms for local stdio servers, excluding server processing time.
- NFR-1.2: Server startup + initialize handshake MUST complete within 10 seconds or timeout.
- NFR-1.3: Memory per idle server connection: one child process handle + two pipe buffers. No polling, no background threads for idle connections.

### NFR-2: Reliability

- NFR-2.1: A crashing or misbehaving MCP server MUST NOT crash aichat.
- NFR-2.2: Malformed JSON-RPC responses MUST be logged and treated as tool errors.
- NFR-2.3: Server stderr MUST be captured and logged at debug level, never mixed with aichat's stdout/stderr.
- NFR-2.4: Notifications from servers (e.g., `notifications/message`, log events) MUST be consumed and discarded (or logged at trace level) without blocking the request/response flow.

### NFR-3: Compatibility

- NFR-3.1: MUST work on Linux, macOS, and Windows.
- NFR-3.2: No new runtime dependencies beyond Rust crates.
- NFR-3.3: MUST handle JSON-RPC responses that include extra fields (forward-compatible parsing).

### NFR-4: Maintainability

- NFR-4.1: Self-contained module: `src/mcp.rs` or `src/mcp/mod.rs`. No MCP logic in `function.rs`, `main.rs`, or client modules.
- NFR-4.2: Integration surface: one insertion point in `ToolCall::eval()` (or its caller) that checks "is this an MCP tool?" and delegates. One insertion point in tool discovery that merges MCP declarations into the tool list.
- NFR-4.3: The module MUST be behind a cargo feature flag (`mcp`) so it can be compiled out for minimal builds.

## Out of Scope

- MCP resource subscriptions (`resources/list`, `resources/read`)
- MCP sampling/completions callbacks (`sampling/createMessage`)
- MCP prompts (`prompts/list`, `prompts/get`)
- SSE or HTTP Streamable transport (stdio only)
- MCP server hosting (aichat *as* an MCP server in Rust)
- Automatic server discovery or registry
- New dispatch abstractions (trait, enum, registry pattern)
- Async refactor of `eval_tool_calls` (that's backlog #3)

## Acceptance Criteria

1. Configure an MCP server in `config.yaml`, run a prompt that triggers a tool call → result returned correctly, no Node.js involved.
2. `aichat --info` shows MCP servers and their tools alongside shell-exec tools.
3. An agent with both shell-exec tools and MCP tools works without conflict or special configuration.
4. A crashing MCP server produces a clear error message returned to the LLM, no panic or hang.
5. The existing shell-exec path is completely unchanged — zero regression.
6. Works in CLI one-shot, REPL, and serve mode.
7. Building with `--no-default-features` (without `mcp` feature) produces a working binary without MCP support.
8. The Node.js MCP server (`mcp/server/index.js`) continues to function for exposing tools to external MCP clients.

## Resolved Design Decisions

1. **FR-2.5 vs FR-1.6 (discovery vs lazy spawn):** Use a **cached tool manifest**. On first use (or `--sync-mcp`), spawn each server, fetch `tools/list`, write the result to a local cache file (`<config-dir>/mcp-cache/<server-name>.json`). On subsequent runs, load declarations from cache without spawning. Server process is only spawned when a cached tool is actually *invoked*. Cache is invalidated by: `--sync-mcp` flag, manual deletion, or config change (command/args hash mismatch).
2. **Namespacing format:** `server__tool` (double underscore). Matches the bridge's existing `formatToolName` convention. Valid JSON key, unambiguous, splittable.
3. **Feature flag:** Single `mcp` cargo feature for now. Split later if/when native MCP server hosting is added.

## Open Questions

None at this time.
