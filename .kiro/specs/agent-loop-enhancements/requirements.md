# Client-Side Agent Loop Enhancements — Requirements

## Summary

Enhance the provider-agnostic client-side agent loop (`run_directive` / `ask_inner`) to support parallel tool execution, bounded recursion, sub-agent delegation via subprocess, live progress reporting, external observability signals, and a built-in planning tool. These enhancements make every LLM provider capable of multi-step agentic work without requiring provider-specific server-side orchestration APIs.

## Context

- Current state: `run_directive` (CLI mode) and `ask_inner` (REPL mode) are recursive async functions. Each turn: call the LLM, get tool_calls in the response, evaluate them sequentially via `eval_tool_calls`, merge results into the next input, recurse. No turn limit. No parallelism. No progress visibility. No sub-agent delegation.
- The OpenAI Responses multi-agent path (`run_multi_agent_directive`) already has: a 64-turn limit (`MAX_CONTINUATION_TURNS`), live progress via `OpenAIResponsesProgress`, trace events via an unbounded channel, and a spinner/heartbeat rendering loop. But this only works with OpenAI.
- The MCP bridge (backlog #1, complete) added async tool calling behind `block_in_place`. This spec makes `eval_tool_calls` fully async, which naturally eliminates the bridge overhead.
- Deployment assumption: aichat runs inside tmux. This informs the observability design (OSC titles, bell notifications, pane visibility).
- Goal: The classic loop becomes competitive with server-side orchestration for any provider that returns tool_calls — OpenRouter, Claude, Cohere, local models via Ollama, etc.

## Design Philosophy

1. **Provider-agnostic.** The enhancements work with any client that implements `chat_completions` / `chat_completions_streaming` and returns `tool_calls`. No provider-specific code paths.
2. **Backward-compatible.** With defaults applied, the loop behaves identically to today for simple single-tool-call exchanges. Enhancements activate when the model uses them (parallel calls, agent tools, planning tool) or when the user configures them (turn budget, trace).
3. **Shared execution layer.** Both `run_directive` and `ask_inner` use the same enhanced execution logic. No divergence between CLI and REPL agent behavior.
4. **Composable with server-side paths.** The client-side loop and OpenAI Responses multi-agent path remain peers — different delegation models, shared tool execution. This spec does not merge them.
5. **Incremental adoption.** Each enhancement is independently useful. Parallel execution alone is valuable without sub-agents. A turn budget is valuable without progress reporting. The spec is ordered by dependency, not by "all or nothing."
6. **Agents as independent entities.** Sub-agents are spawned as separate aichat processes with their own PID, session, status file, and lifecycle. This gives each agent first-class observability and enables future evolution toward fully independent peer agents (Model B) without architectural rework.

## Functional Requirements

### FR-1: Async Tool Execution

- FR-1.1: `eval_tool_calls` MUST become an async function (`async fn eval_tool_calls`).
- FR-1.2: The MCP bridge's current `block_in_place` bridge MUST be replaced with a direct `.await` on the async MCP call path.
- FR-1.3: Shell-exec tool calls MUST be wrapped in `tokio::task::spawn_blocking` to avoid blocking the async runtime.
- FR-1.4: The function signature MUST remain compatible: accept `Vec<ToolCall>`, return `Result<Vec<ToolResult>>`. The `GlobalConfig` parameter remains.
- FR-1.5: All existing callers (`call_chat_completions`, `call_chat_completions_streaming`, `eval_tool_calls_preserving_results`) MUST be updated to `.await` the result.

### FR-2: Parallel Tool Execution

- FR-2.1: When the LLM returns multiple tool_calls in a single response, they MUST be executed concurrently by default.
- FR-2.2: Concurrency MUST use `futures::future::join_all` (or equivalent) over spawned tasks, one per tool call.
- FR-2.3: Each tool call MUST be independently failable — one tool's failure MUST NOT prevent other tools from completing. Failed tools return the existing error JSON pattern.
- FR-2.4: Tool result ordering MUST be preserved — results are returned in the same order as the input calls (matched by position or `ToolCall.id`).
- FR-2.5: A configurable concurrency limit MUST be supported to prevent resource exhaustion (e.g., 8 simultaneous shell subprocesses). Default: 8.
- FR-2.6: The existing dedup logic (`ToolCall::dedup`) and infinite-loop detection MUST run before parallel dispatch.

### FR-3: Max-Turns Budget

- FR-3.1: The agent loop MUST enforce a configurable maximum number of turns (tool-call → result → next-call cycles).
- FR-3.2: Default budget MUST be 20 turns for the classic loop.
- FR-3.3: The budget MUST be configurable via `config.yaml` under an `agent_loop` key: `max_turns: <integer>`.
- FR-3.4: When the budget is exhausted, the loop MUST stop, emit a warning to stderr, and return the accumulated token usage. It MUST NOT error/bail — the conversation up to that point is valid.
- FR-3.5: The warning message MUST include the configured limit and suggest the config key to increase it.
- FR-3.6: The budget MUST be per top-level invocation (one `run_directive` or `ask_inner` call), not global across the session.
- FR-3.7: Sub-agent processes (FR-4) have their own independent budget (each is a separate aichat invocation with its own `max_turns`).

### FR-4: Agent-as-Tool (Sub-Agent Delegation)

- FR-4.1: When a `ToolCall` targets a function with `agent: true` in its `FunctionDeclaration`, the eval path MUST spawn a sub-agent as a **separate aichat process** rather than shelling out to a generic binary.
- FR-4.2: The sub-agent MUST be invoked as: `aichat --agent <agent_name> [--model <model>] "<task_message>"` where `<task_message>` is derived from the tool call arguments.
- FR-4.3: The sub-agent process MUST inherit the parent's config directory (same `AICHAT_CONFIG_DIR`) so it has access to the same agents, tools, and MCP servers.
- FR-4.4: The sub-agent MUST run with its own independent turn budget, session, status file, and observability signals. It is a first-class aichat instance.
- FR-4.5: Sub-agents MUST NOT have access to the parent's conversation history — they receive only the arguments passed to them via the tool call.
- FR-4.6: Sub-agent output MUST be captured from stdout and returned as: `{"output": "<final_text>"}` to match the existing shell-exec tool result pattern.
- FR-4.7: If a sub-agent process exits non-zero (including budget exhaustion), the parent MUST capture stderr and return it as: `{"error": {"type": "agent_error", "message": "<stderr>"}}`.
- FR-4.8: Sub-agent nesting MUST be bounded. The parent MUST pass its current depth via environment variable (`AICHAT_AGENT_DEPTH=<n>`). The sub-agent reads this, increments it, and refuses to spawn further sub-agents if it exceeds `max_agent_depth` (default: 3).
- FR-4.9: Multiple sub-agent invocations from the same turn MUST execute in parallel (subject to FR-2.5 concurrency limit), as they are independent processes.
- FR-4.10: The parent MUST block on sub-agent completion (Model A — synchronous delegation). The parent's tool call is not resolved until the sub-agent exits. Future evolution to non-blocking delegation (Model B) is a separate effort.

### FR-5: Progress and Trace Reporting

- FR-5.1: The classic agent loop MUST emit structured progress events during multi-turn execution.
- FR-5.2: Progress events MUST include at minimum:
  - Turn start: `"turn {n}/{max}"` with the tool calls being made
  - Tool execution: tool name, start time
  - Tool completion: tool name, duration, success/failure
  - Sub-agent start/finish: agent name, PID
  - Budget warning: when approaching the limit (e.g., turn 18/20)
- FR-5.3: Progress MUST be surfaced via:
  - Spinner message updates (when stdout is a terminal and streaming)
  - Trace lines printed to stderr (when `agent_loop.show_trace: true`)
  - Debug log entries (always, for `--log-level debug`)
- FR-5.4: The progress infrastructure MUST be decoupled from the execution logic — a `Progress` type that the loop pushes events into, and a renderer that consumes them.
- FR-5.5: In REPL mode, progress MUST NOT interfere with the prompt or input line.
- FR-5.6: When `show_trace` is false and stdout is not a terminal (piped/redirected), NO progress output MUST be emitted. Silent execution for scripting.
- FR-5.7: When stdout is a terminal, the loop MUST emit OSC 0/2 escape sequences to set the terminal title reflecting current agent state. Title formats:
  - Idle (REPL awaiting input): `aichat: idle`
  - Working: `aichat: turn {n}/{max} | {tool_names}`
  - Waiting for input (REPL, after agent work completes): `aichat: waiting for input`
  - Done (CLI mode, task complete): `aichat: done`
  - Budget exhausted: `aichat: turn limit reached`
  This enables tmux pane title display, Herdr screen-manifest detection, and terminal tab labeling without any configuration.
- FR-5.8: The loop MUST maintain an atomic JSON status file at `$XDG_RUNTIME_DIR/aichat-<pid>.json` (falling back to `/tmp/aichat-<pid>.json`) updated on every state transition. The file MUST contain:
  ```json
  {
    "pid": 12345,
    "state": "working",
    "turn": 3,
    "max_turns": 20,
    "active_tools": ["execute_command", "fs_cat"],
    "model": "openrouter/anthropic/claude-sonnet-4",
    "agent": "researcher",
    "depth": 1,
    "elapsed_s": 4.2,
    "session": "my-project",
    "updated_at": "2026-08-24T14:32:01Z"
  }
  ```
  The file MUST be deleted on normal process exit. Each aichat process (including sub-agents) writes its own independent status file keyed by PID. External tools enumerate all `aichat-*.json` files to get the full fleet view.
- FR-5.9: On discrete attention-requiring events, the loop MUST emit terminal notification signals:
  - BEL character (`\x07`) — universal signal that tmux uses to highlight the pane/window in the status bar (`monitor-bell`).
  - OSC 777 desktop notification (`\033]777;notify;aichat;<message>\007`) — triggers native desktop notifications on supporting terminals (Ghostty, iTerm2, VS Code terminal, rxvt-unicode).
  Events that trigger notifications:
  - Agent loop completed (all turns done, final output ready)
  - Budget exhausted (partial result, needs attention)
  - REPL waiting for input after agent work (task done, user's turn)
  Events that MUST NOT trigger notifications:
  - Mid-loop between turns (too noisy)
  - Individual tool completions
  Sub-agent processes manage their own notifications independently (they are separate aichat instances). In Model A (blocking), sub-agents typically run with stdout captured (not a tty), so their notifications are naturally suppressed.
  Notifications MUST be suppressible via config: `agent_loop.notify: false`.
- FR-5.10: OSC title updates (FR-5.7), status file (FR-5.8), and notifications (FR-5.9) MUST be independently configurable:
  ```yaml
  agent_loop:
    osc_title: true       # FR-5.7, default: true (when stdout is tty)
    status_file: true     # FR-5.8, default: true
    notify: true          # FR-5.9, default: true (when stdout is tty)
  ```
  All default to enabled. Disabling any one MUST NOT affect the others.

### FR-6: Built-in Planning Tool

- FR-6.1: A built-in pseudo-tool named `_plan` MUST be available when agent-loop tools are active.
- FR-6.2: The `_plan` tool MUST accept a single string argument (`plan` or `thought`) containing the model's reasoning/decomposition.
- FR-6.3: The content of a `_plan` call MUST be:
  - Appended to the conversation context for the next turn (so the model can reference its own plan)
  - NOT displayed to the user in the output stream
  - Logged at debug level
  - Reported in trace output (FR-5) when `show_trace` is enabled
- FR-6.4: `_plan` MUST NOT count as a "real" tool call for the purpose of turn budget consumption — a turn that only contains `_plan` calls MUST still be counted as one turn, but a turn that mixes `_plan` with real tools counts as one turn.
- FR-6.5: `_plan` MUST be injected into the tool list automatically when the role/agent has other tools configured. It MUST NOT appear when no tools are configured (pure chat mode).
- FR-6.6: The `_plan` tool MUST be excludable via config: `agent_loop.planning_tool: false`.
- FR-6.7: The `FunctionDeclaration` for `_plan` MUST have `agent: false` (it is not a sub-agent).

### FR-7: Tool Output Handles (Large Result Capping)

- FR-7.1: When a tool's output exceeds a configurable size threshold, the full output MUST be written to a temporary file and the model MUST receive a bounded preview instead of the full content.
- FR-7.2: Default threshold MUST be 16 KB. Configurable via `agent_loop.tool_output_limit` (bytes).
- FR-7.3: The preview returned to the model MUST contain:
  - The first N bytes/lines of the output (up to the threshold)
  - The full path to the temp file containing the complete output
  - The total byte count
  - A shape hint: line count for text, top-level keys for JSON
- FR-7.4: The format MUST be: `{"preview": "<truncated content>", "full_output_path": "/tmp/aichat-tool-<id>.out", "total_bytes": 524288, "hint": "2847 lines"}`
- FR-7.5: Temp files MUST be cleaned up on process exit (same cleanup path as status files).
- FR-7.6: The model can access the full output via its existing tools (e.g., `fs_cat` with offset/limit, `execute_command` with grep/head/tail).
- FR-7.7: The threshold MUST NOT apply to `_plan` tool results or error results — only to successful tool outputs.

### FR-8: Structured Workflow Tool (Multi-Phase Fan-Out)

- FR-8.1: A built-in pseudo-tool named `_workflow` MUST be available when agent-loop tools are active and sub-agent delegation is configured (i.e., at least one `agent: true` tool exists).
- FR-8.2: The `_workflow` tool MUST accept a JSON argument defining a sequence of phases, each containing one or more parallel tasks:
  ```json
  {
    "phases": [
      {"tasks": [{"agent": "researcher", "prompt": "find X"}, {"agent": "researcher", "prompt": "find Y"}]},
      {"tasks": [{"agent": "writer", "prompt": "synthesize: {{prev}}"}]}
    ]
  }
  ```
- FR-8.3: Tasks within a phase MUST execute in parallel (subject to `max_concurrency`). Phases MUST execute sequentially — phase N+1 starts only after all tasks in phase N complete.
- FR-8.4: The template variable `{{prev}}` in task prompts MUST be replaced with a formatted summary of all results from the previous phase. If omitted from a phase 2+ task, previous results MUST be appended automatically.
- FR-8.5: Maximum phases MUST be capped (default: 5). Maximum tasks per phase MUST be capped (default: 8).
- FR-8.6: The final phase's results MUST be returned as the `_workflow` tool's output to the parent loop.
- FR-8.7: `_workflow` MUST count as a single tool call for turn budget purposes (one turn consumed regardless of internal phases/tasks).
- FR-8.8: Individual task failures within a phase MUST NOT abort the entire workflow — failed tasks return error results that flow into `{{prev}}` for subsequent phases to handle.
- FR-8.9: `_workflow` MUST be excludable via config: `agent_loop.workflow_tool: false`. Default: true (when agent tools exist).
- FR-8.10: The `FunctionDeclaration` for `_workflow` MUST have `agent: false` (it orchestrates agents but is not itself an agent).

## Configuration

### New `agent_loop` config section

```yaml
agent_loop:
  max_turns: 20              # FR-3.3
  max_concurrency: 8         # FR-2.5
  max_agent_depth: 3         # FR-4.8
  show_trace: false          # FR-5.3
  planning_tool: true        # FR-6.6
  osc_title: true            # FR-5.7
  status_file: true          # FR-5.8
  notify: true               # FR-5.9
  tool_output_limit: 16384   # FR-7.2 (bytes)
  workflow_tool: true         # FR-8.9
```

All fields optional with the defaults shown above. Applied globally; agents MAY override `max_turns` in their own agent config.

### Environment variable overrides

- `AICHAT_AGENT_LOOP_MAX_TURNS` — overrides `max_turns`
- `AICHAT_AGENT_LOOP_SHOW_TRACE` — overrides `show_trace` (values: `true`/`false`/`1`/`0`)
- `AICHAT_AGENT_DEPTH` — set by parent when spawning sub-agents (read-only from user perspective)

## Non-Functional Requirements

- NFR-1: Parallel tool execution MUST NOT increase memory usage by more than O(n) where n is the number of concurrent tools (no unbounded buffering).
- NFR-2: The enhanced loop MUST add less than 5ms of overhead per turn compared to the current sequential implementation when only one tool call is present.
- NFR-3: Progress reporting MUST NOT introduce observable latency to tool execution (fire-and-forget channel, not blocking).
- NFR-4: The implementation MUST compile and pass tests with `--no-default-features` (no MCP). MCP-specific parallel paths are behind the `mcp` feature gate.
- NFR-5: All new public interfaces MUST have doc comments.
- NFR-6: The existing 298 tests MUST continue to pass. New tests MUST cover: parallel execution correctness, turn budget enforcement, sub-agent basic delegation, and planning tool injection/suppression.

## Out of Scope (this spec)

- Human-in-the-loop pause points (future consideration)
- Merging with the OpenAI Responses multi-agent path
- Changes to how tools are defined or discovered (that's backlog #1, done)
- Structured output enforcement
- Streaming tool results back to the LLM mid-execution
- Non-blocking delegation / Model B (sub-agents signal completion asynchronously, parent doesn't block) — future evolution once Model A is proven
- Tool output routing / destination control (backlog #4)

## Future Direction: Model B (Non-Blocking Delegation)

This spec implements Model A: the parent blocks on sub-agent completion (synchronous delegation). The subprocess boundary established here is the foundation for Model B, where:

- Sub-agents run in their own tmux panes (visible, independently observable)
- The parent doesn't block — it delegates and its turn ends
- Completion is signaled via status file state transitions or a coordination protocol
- External tools (Herdr, Agent Deck) can observe and manage sub-agents independently
- Sub-agents can persist across delegations (`--session` flag)

The subprocess model (FR-4) means Model B requires no architectural rework — only changing "block until process exits" to "spawn in background, poll for completion on next turn."
