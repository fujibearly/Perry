# Project Progress

## Current State (2026-08-24)

**Branch:** `feat/agent-loop-enhancements` (off `feat/rust-mcp-bridge`)  
**Version:** v0.31.0-fork.9  
**Last commit:** `ae68429` — feat: agent loop Phase C — parallel tool execution

## Commit History (feat/agent-loop-enhancements)

1. `65ef41c` — Phase A: config, module skeleton, async eval, raw LLM call
2. `aeff42b` — Phase B: iterative loop replaces recursion, turn budget enforced
3. `012a7a2` — docs: .kiro specs/steering
4. `ae68429` — Phase C: parallel tool execution

## What's Done

### Backlog #1: Rust MCP Bridge ✓

Full implementation committed on `feat/rust-mcp-bridge`. 1272 lines, 298 tests passing.

Key decisions:
- **Option C** — MCP tools look identical to shell-exec. No new abstractions.
- **Cached manifests** — Tool schemas cached to disk, invalidated by config hash.
- **Feature flag:** `mcp` (default on).

Spec at: `.kiro/specs/rust-mcp-bridge/`

---

### Backlog #3: Client-Side Agent Loop Enhancements — In Progress

**Spec:** `.kiro/specs/agent-loop-enhancements/` (requirements, design, tasks)

#### Phase A ✓ — Foundation

- `AgentLoopConfig` struct (10 fields: max_turns, max_concurrency, max_agent_depth, show_trace, planning_tool, osc_title, status_file, notify, tool_output_limit, workflow_tool)
- `src/agent_loop.rs` module: types, progress tracker, plan tool declaration
- `call_chat_completions_raw` / `_streaming_raw` — return raw `Vec<ToolCall>` for the loop to execute
- `eval_tool_calls_async` / `eval_single_tool_async` — async tool dispatch (MCP via await, shell via spawn_blocking)
- `ToolCall::eval_shell()` — extracted shell-exec path
- `call_mcp_tool_async` made public
- `Default` derived on `JsonSchema`

#### Phase B ✓ — Core Loop

- `agent_loop::run()` — iterative `for turn in 1..=max_turns` loop replacing `#[async_recursion]`
- `run_directive` (main.rs) → delegates to `agent_loop::run()`
- `ask_inner` (repl/mod.rs) → delegates to `agent_loop::run()`
- Turn budget enforced with stderr warning on exhaustion
- Progress events: TurnStart, LoopComplete, BudgetWarning, BudgetExhausted
- Session autoname/compress preserved in REPL path

#### Phase C ✓ — Parallel Execution

- `eval_tool_calls_parallel` — concurrent dispatch via `join_all` + semaphore (bounded at `max_concurrency`)
- Per-tool progress events (ToolStart, ToolComplete) with timing
- Active tool tracking on `AgentLoopProgress`
- MCP pool safety fix: parallel calls to same server spawn additional connections instead of failing
- Result ordering preserved regardless of completion order

#### Phase D — Observability (next)

- Progress rendering (spinner + trace lines)
- OSC terminal title updates
- JSON status file for external tools
- BEL + OSC 777 notifications on completion

#### Phase E — Intelligence (future)

- `_plan` pseudo-tool injection and handling
- Sub-agent subprocess delegation (`agent: true` → spawn aichat process)
- `_workflow` structured multi-phase fan-out tool

#### Phase F — Polish (future)

- `--info` display updates
- Full test suite for new features
- Documentation

## Architecture Decisions Log

| Decision | Rationale |
|----------|-----------|
| Don't merge server-side and client-side agent loops | Different delegation models. Shared tool execution layer, separate orchestration. |
| Iterative loop (not recursive) | Trivial budget enforcement, no stack growth, natural progress reporting. |
| Sub-agents as subprocess (not in-process) | Each agent gets its own PID, status file, observability. Process boundary enables crash isolation and future Model B (non-blocking delegation). |
| Parallel by default | Single-tool turns have zero overhead (semaphore permits 8, only 1 used). Multi-tool turns get automatic speedup. |
| MCP pool spawns extra connections for parallel | Simpler than a connection queue. MCP servers are lightweight; extra connections are fine. |
| Tool output handles (FR-7, future) | Prevents context blowout from large tool results. Write to file, pass preview + path. |
| Workflow tool (FR-8, future) | Structured fan-out for multi-phase tasks. Built on top of sub-agent subprocess model. |
| OSC title + status file + bell (FR-5.7-5.9) | Makes aichat observable by tmux, Herdr, Agent Deck without custom integration. |
| Skip Gemini Interactions API | OpenRouter proxies Gemini through OpenAI-compatible format. The client-side loop makes this sufficient. |

## Branch Status

- `main` — upstream fork at v0.31.0-fork.9
- `rc-branch` — release candidate
- `feat/rust-mcp-bridge` — Backlog #1, complete (ready to merge)
- `feat/agent-loop-enhancements` — Backlog #3, Phases A-C complete (active)

## Environment Reminders

- Production aichat: `/usr/bin/aichat` (v0.30.0), config at `~/.config/aichat/`
- Dev binary: `~/projects/aichat/target/release/aichat`
- To test without conflicting: `AICHAT_CONFIG_DIR=/tmp/aichat-test`
- Live functions (don't touch): `~/clones/llm-functions`
- Dev functions (safe): `~/projects/llm-functions`
