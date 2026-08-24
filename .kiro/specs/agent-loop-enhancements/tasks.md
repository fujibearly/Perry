# Client-Side Agent Loop Enhancements — Implementation Tasks

## Task 1: Configuration — `AgentLoopConfig` struct and parsing

**Files:** `src/config/mod.rs`, `config.example.yaml`

1. Add `AgentLoopConfig` struct with all fields (max_turns, max_concurrency, max_agent_depth, show_trace, planning_tool, osc_title, status_file, notify) and `Default` impl.
2. Add to `Config`:
   ```rust
   #[serde(default)]
   pub agent_loop: AgentLoopConfig,
   ```
3. Apply environment variable overrides in `Config::init()` (after YAML load):
   - `AICHAT_AGENT_LOOP_MAX_TURNS` → `self.agent_loop.max_turns`
   - `AICHAT_AGENT_LOOP_SHOW_TRACE` → `self.agent_loop.show_trace`
4. Read `AICHAT_AGENT_DEPTH` at startup — store as a process-level value (not in config, since it's not user-configurable). Used by sub-agent depth checking.
5. Add commented `agent_loop` section to `config.example.yaml` showing all defaults.
6. Verify: `cargo build` succeeds. Existing configs without `agent_loop` load correctly (defaults applied). Config with explicit values parses correctly.

---

## Task 2: Module skeleton — `src/agent_loop.rs`

**Files:** `src/agent_loop.rs`, `src/main.rs`

1. Create `src/agent_loop.rs` with:
   - Module doc comment explaining purpose
   - Public types: `AgentLoopParams`, `AgentLoopOutput`, `AgentLoopEvent`, `AgentLoopProgress`, `AgentLoopSnapshot`
   - Placeholder `pub async fn run(input: Input, params: AgentLoopParams<'_>) -> Result<AgentLoopOutput>` that just does a single LLM call (no loop yet) — transitional scaffold
   - `plan_tool_declaration()` function returning the `_plan` FunctionDeclaration
2. Add `mod agent_loop;` to `src/main.rs`.
3. Verify: `cargo build` succeeds. New module compiles. No behavior change yet.

---

## Task 3: Async `eval_tool_calls` — make tool execution async

**Files:** `src/function.rs`, `src/client/common.rs`, `src/mcp.rs`

1. Create `pub async fn eval_tool_calls_async(config: &GlobalConfig, calls: Vec<ToolCall>) -> Result<Vec<ToolResult>>`:
   - Same logic as current `eval_tool_calls_with_options` but:
   - Shell-exec calls wrapped in `tokio::task::spawn_blocking`
   - MCP calls use direct `.await` (remove `block_in_place`)
   - Still sequential at this stage (parallel comes in Task 4)
2. Add `ToolCall::eval_shell(&self, config: &GlobalConfig) -> Result<Value>`:
   - Extract the shell-exec path from current `ToolCall::eval()` into its own method
   - The remaining `ToolCall::eval()` stays as-is for backward compat (sync callers)
3. In `src/mcp.rs`, add `pub async fn call_mcp_tool_async(...)` alongside existing `call_mcp_tool`:
   - Direct async version without `block_in_place`
   - Keep the sync `call_mcp_tool` as a wrapper for backward compat
4. Update `call_chat_completions` and `call_chat_completions_streaming` in `src/client/common.rs`:
   - Change `eval_tool_calls(...)` call to `eval_tool_calls_async(...).await`
   - These functions are already async, so this is straightforward
5. Verify: `cargo test` — all 298 existing tests pass. Behavior identical to before (still sequential, but now async-structured).

---

## Task 4: Parallel tool execution

**Files:** `src/agent_loop.rs`

1. Implement `pub async fn eval_tool_calls_parallel(config: &GlobalConfig, calls: Vec<ToolCall>, abort_signal: AbortSignal, progress: &AgentLoopProgress) -> Result<Vec<ToolResult>>`:
   - Run `ToolCall::dedup()` first (existing infinite-loop detection)
   - Create `Arc<Semaphore>` with `max_concurrency` permits
   - Spawn each tool call as an async task via `futures::future::join_all`
   - Each task: acquire permit → dispatch (MCP async / sub-agent subprocess / shell spawn_blocking) → emit ToolStart/ToolComplete events → release permit → return ToolResult
   - Collect results preserving input order
   - Handle the "all null" case (existing behavior: return empty vec)
2. Implement `eval_single_tool_async()` dispatcher:
   - Route 1: MCP tools → `call_mcp_tool_async`
   - Route 2: Agent tools → `eval_agent_tool_subprocess`
   - Route 3: Shell-exec → `spawn_blocking` + `eval_shell`
3. Each tool failure is independent — caught and converted to the error JSON, does not cancel siblings.
4. Add `futures` crate to `Cargo.toml` if not already present (for `join_all`).
5. Verify: Unit test — 3 tools with artificial delays (50ms, 100ms, 50ms). Total time should be ~100ms (parallel) not ~200ms (sequential). Results in correct order.

---

## Task 5: Iterative agent loop — core `agent_loop::run()`

**Files:** `src/agent_loop.rs`

1. Implement the full iterative loop in `agent_loop::run()`:
   ```
   for turn in 1..=max_turns:
     emit TurnStart
     call LLM (streaming or non-streaming based on input.stream())
     config.write().after_chat_completion(...)
     accumulate usage, track last_text
     if no tool_calls → emit LoopComplete, return AgentLoopOutput with final_text
     partition _plan calls from real calls
     handle plan calls (emit PlanReceived, log at debug)
     eval_tool_calls_parallel(real_calls)
     merge tool results + plan context into next input
     emit BudgetWarning if turn >= max_turns - 2
   budget exhausted → emit BudgetExhausted, eprintln warning, return with last_text
   ```
2. Handle the `code_mode` / `extract_code` concern: if `code_mode && !IS_STDOUT_TERMINAL`, extract code from the *final* text output (last turn only, after the loop ends).
3. The LLM call inside the loop uses `call_chat_completions_raw` (Task 12) which returns raw tool_calls without evaluating them.
4. Verify: Integration test with a mock client that returns 3 turns of tool calls then a final text. Confirm loop runs exactly 4 iterations (3 tool turns + 1 final).

---

## Task 6: Refactor callers — `run_directive` and `ask_inner`

**Files:** `src/main.rs`, `src/repl/mod.rs`

1. Refactor `run_directive` in `src/main.rs`:
   - If the input has no tools configured: keep current single-call behavior (simple single-turn path)
   - If tools are configured: create `AgentLoopParams`, call `agent_loop::run()`, return the output usage
   - Remove `#[async_recursion]` from `run_directive`
   - The multi-agent path (`run_multi_agent_directive`) is unchanged
2. Refactor `ask_inner` in `src/repl/mod.rs`:
   - Same pattern: delegate to `agent_loop::run()` when tools are present
   - Preserve session compression logic: call `Config::maybe_compress_session` after the loop returns
   - Preserve `Config::maybe_autoname_session` after the loop returns
   - Remove `#[async_recursion]` from `ask_inner`
3. Update the `before_chat_completion` call: called once at loop entry (turn 1). The loop handles subsequent turns internally.
4. Verify: `cargo test` all passing. Manual test: CLI mode with tools → works. REPL mode with tools → works. No-tools mode → unchanged.

---

## Task 7: Progress reporting infrastructure

**Files:** `src/agent_loop.rs`, `src/main.rs`, `src/repl/mod.rs`

1. Implement `AgentLoopProgress`:
   - `live() -> (Self, UnboundedReceiver<AgentLoopEvent>)` — creates the channel pair
   - `emit(&self, event: AgentLoopEvent)` — fire-and-forget send
   - `snapshot(&self) -> AgentLoopSnapshot` — for spinner heartbeat
2. Implement `AgentLoopSnapshot` struct:
   ```rust
   pub struct AgentLoopSnapshot {
       pub current_turn: usize,
       pub max_turns: usize,
       pub active_tools: Vec<String>,
       pub elapsed: Duration,
   }
   ```
3. Implement spinner/trace rendering in callers:
   - In `run_directive` (main.rs): wrap `agent_loop::run()` in a `tokio::select!` loop with heartbeat + event_rx, same pattern as `run_multi_agent_directive`
   - Trace format: `"  [turn {n}/{max}] {event_description}"`
   - Spinner format: `"Turn {n}/{max} | executing: {tool_name} ({elapsed}s)"`
   - Only render trace when `config.read().agent_loop.show_trace` is true
   - Only render spinner when stdout is a terminal
   - In REPL: same pattern, using the existing spinner infrastructure
4. Implement `format_agent_loop_trace_event(event: &AgentLoopEvent) -> String` for trace lines.
5. Verify: Run with `AICHAT_AGENT_LOOP_SHOW_TRACE=true` and tools configured → trace lines appear on stderr. Without the flag → silent.

---

## Task 8: Planning tool — `_plan` injection and handling

**Files:** `src/agent_loop.rs`, `src/config/input.rs`

1. Implement `plan_tool_declaration()` as specified in design.
2. In `agent_loop::run()`, before calling the LLM:
   - If `config.read().agent_loop.planning_tool` is true and input has tools
   - Inject `_plan` into the function declarations sent to the LLM (if not already present)
3. Implement `partition_plan_calls(calls: Vec<ToolCall>) -> (Vec<ToolCall>, Vec<ToolCall>)`:
   - Returns (plan_calls, real_calls)
4. Implement plan context injection:
   - `_plan` call results are returned as tool results with `output: "acknowledged"`
   - The plan text is available in conversation context for the next turn (via tool result message)
   - NOT rendered to the user
5. Emit `AgentLoopEvent::PlanReceived { content }` for trace output.
6. Log plan content at `debug!` level.
7. Implement the config flag to disable: `planning_tool: false` → `_plan` is not injected.
8. Verify: Test with a model that uses `_plan` → plan content appears in trace, not in output. Test with `planning_tool: false` → `_plan` not in tool list.

---

## Task 9: Sub-agent delegation — subprocess model

**Files:** `src/agent_loop.rs`, `src/function.rs`

1. Implement `is_agent_tool(config: &GlobalConfig, tool_name: &str) -> bool`:
   - Check if the tool's `FunctionDeclaration` has `agent: true`
   - Check via the current agent's functions (if in agent context) or global functions
2. Implement `eval_agent_tool_subprocess(config, call) -> Result<Value>`:
   - Read `AICHAT_AGENT_DEPTH` env var (default 0), check against `max_agent_depth`
   - Resolve agent name from the tool call name
   - Build command: `std::env::current_exe()? --agent <name> "<arguments>"`
   - Set env: `AICHAT_AGENT_DEPTH=<current+1>`, inherit `AICHAT_CONFIG_DIR`
   - Capture stdout (piped), stderr (piped)
   - Spawn via `tokio::process::Command`, await `wait_with_output()`
   - On success: return `json!({"output": stdout_text})`
   - On failure: return `json!({"error": {"type": "agent_error", "message": stderr_text}})`
3. Emit `AgentLoopEvent::SubAgentStart { agent_name, pid }` before awaiting the process.
4. Emit `AgentLoopEvent::SubAgentComplete { agent_name, pid, duration, success }` after.
5. Integrate into `eval_single_tool_async()`:
   - After MCP check, before shell-exec: `if is_agent_tool(...) { return eval_agent_tool_subprocess(...).await; }`
6. Verify: Create a test agent with `agent: true` function. Invoke from parent → sub-agent process spawns, runs its own loop, returns stdout. Verify depth enforcement by setting AICHAT_AGENT_DEPTH near max.

---

## Task 10: Max-turns budget — enforcement and partial output

**Files:** `src/agent_loop.rs`

1. The loop bound (`for turn in 1..=max_turns`) enforces the budget. Ensure correct behavior at boundary:
   - Track `last_text` across iterations (last LLM response text before tool execution)
   - On budget exhaustion, return `last_text` as `final_text` (partial but valid output)
2. Implement budget warning emission at turn `max_turns - 2`.
3. Implement the stderr warning message:
   ```
   Warning: Agent loop reached the 20-turn limit without completing.
   Increase with `agent_loop.max_turns` in config.yaml or AICHAT_AGENT_LOOP_MAX_TURNS=N.
   ```
4. Per-agent budget override: add optional `max_turns` to `AgentConfig`. When a sub-agent is spawned, it reads its own agent config and uses that value (or falls back to global default). No special plumbing needed — it's a separate aichat process reading its own config.
5. Verify: Unit test with a mock client that always returns tool_calls → loop stops at max_turns, returns partial text, warning printed.

---

## Task 11: MCP bridge async cleanup

**Files:** `src/mcp.rs`, `src/function.rs`

1. Make `call_mcp_tool_async` the primary entry point (it already exists from Task 3).
2. The sync wrapper `call_mcp_tool` uses `block_in_place` — keep it for code paths outside the agent loop (e.g., OpenAI Responses module's `execute_function_calls`).
3. In `eval_single_tool_async`, call `call_mcp_tool_async` directly — no `block_in_place`.
4. Ensure `call_mcp_tool_async` properly acquires/releases the MCP connection pool lock without holding it across the await.
5. Verify: MCP tool calls work in both the new agent loop (async path) and the existing OpenAI Responses module (sync wrapper path). No deadlocks under parallel execution.

---

## Task 12: `call_chat_completions_raw` — LLM call without tool eval

**Files:** `src/client/common.rs`

1. Add `pub async fn call_chat_completions_raw(input: &Input, print: bool, extract_code: bool, client: &dyn Client, abort_signal: AbortSignal) -> Result<(ChatCompletionsOutput, Vec<ToolCall>)>`:
   - Same as `call_chat_completions` but returns raw `tool_calls` from `ChatCompletionsOutput` instead of evaluated `Vec<ToolResult>`
   - Does not call `eval_tool_calls` — the caller (agent_loop) handles execution
   - Still handles print and extract_code for the text portion
2. Add `pub async fn call_chat_completions_streaming_raw(input: &Input, client: &dyn Client, abort_signal: AbortSignal) -> Result<(ChatCompletionsOutput, Vec<ToolCall>)>`:
   - Same streaming path but returns raw tool_calls
3. These are called by `agent_loop::run()`. The existing `call_chat_completions` / `call_chat_completions_streaming` remain unchanged for backward compat.
4. Verify: `cargo build`. Existing paths unchanged. New `_raw` variants return tool_calls correctly.

---

## Task 13: `--info` display and documentation

**Files:** `src/config/mod.rs`, `config.example.yaml`

1. Add agent_loop config to `--info` output:
   ```
   Agent Loop:
     max_turns: 20
     max_concurrency: 8
     max_agent_depth: 3
     planning_tool: true
     show_trace: false
     osc_title: true
     status_file: true
     notify: true
   ```
2. Update `config.example.yaml` with the full `agent_loop` section and comments explaining each field.
3. Verify: `cargo run -- --info` displays agent_loop section.

---

## Task 14: Tests

**Files:** `src/agent_loop.rs` (unit tests module), integration test files

1. **Turn budget test:** Mock client returns tool_calls every turn. Confirm loop stops at `max_turns`, emits `BudgetExhausted`, returns partial output.
2. **Parallel execution test:** 3 tool calls with timing assertions — total time ≈ max(individual times), not sum.
3. **Parallel ordering test:** 3 tool calls with different delays — results returned in original call order regardless of completion order.
4. **Planning tool partition test:** Mix of `_plan` and real tool calls → correctly separated.
5. **Planning tool injection test:** When `planning_tool: true` and tools exist → `_plan` in declarations. When `false` → absent.
6. **Sub-agent depth test:** Set `AICHAT_AGENT_DEPTH` to max value → sub-agent spawn refused with error result.
7. **Sub-agent integration test:** Define a minimal test agent, invoke via subprocess, verify output captured correctly.
8. **Config parsing test:** agent_loop section with all fields, partial fields, absent section → correct defaults.
9. **Environment override test:** Set `AICHAT_AGENT_LOOP_MAX_TURNS=5` → config reflects it.
10. **Dedup/infinite-loop test:** Existing dedup behavior preserved in parallel path.
11. **Single tool call regression:** One tool call per turn → works correctly, no overhead from parallelism infrastructure.
12. **Status file test:** Verify file created at expected path, contains valid JSON with correct fields, deleted on exit.
13. Verify: `cargo test` — all new tests pass, all 298 existing tests still pass.

---

## Task 15: External observability — OSC title, status file, notifications

**Files:** `src/agent_loop.rs`, `src/repl/mod.rs`

1. Implement `update_terminal_title(state: &str)`:
   - Guard: only emit when `IS_STDOUT_TERMINAL` is true and `config.agent_loop.osc_title` is true
   - Write `\x1b]0;{state}\x07` to stderr
   - Title formats: idle, turn N/M | tools, waiting for input, done, turn limit reached
2. Implement `write_status_file(status: &AgentLoopStatus)`:
   - Path: `$XDG_RUNTIME_DIR/aichat-<pid>.json`, fallback to `/tmp/aichat-<pid>.json`
   - Atomic write: create `.tmp` file, write JSON, rename over target
   - `AgentLoopStatus` struct: pid, state, turn, max_turns, active_tools, model, agent (Option), depth, elapsed_s, session, updated_at
   - Guard: only write when `config.agent_loop.status_file` is true
3. Implement `cleanup_status_file()`:
   - Delete the status file on normal exit
   - Register cleanup via a Drop guard and in the abort signal handler
4. Implement `notify_terminal(title: &str, message: &str)`:
   - Guard: only emit when `IS_STDOUT_TERMINAL` is true and `config.agent_loop.notify` is true
   - Emit BEL (`\x07`) + OSC 777 (`\x1b]777;notify;{title};{message}\x07`) to stderr
   - Fire on: LoopComplete, BudgetExhausted, REPL waiting for input after agent work
   - Do NOT fire on: mid-loop turns, individual tool completions
5. Integrate all three into the event renderer (`render_event`):
   - TurnStart / ToolStart / ToolComplete → title + status file
   - LoopComplete → title ("done") + status file + notify
   - BudgetExhausted → title ("turn limit reached") + status file + notify
6. In REPL mode: set title to "idle" when returning to prompt, "waiting for input" + notify after agent work completes.
7. Verify: Run in tmux → bell fires on completion, title updates visible, status file created/deleted. Sub-agents (piped stdout) don't emit OSC/bell but do write status files.

---

## Execution Order

Tasks are ordered by dependency:

```
T1 (config) ─────┐
                  ├──→ T2 (module skeleton) ──→ T5 (iterative loop) ──→ T6 (refactor callers)
T3 (async eval) ──┘                                     │
      │                                                  │
      ├──→ T4 (parallel) ──→ T5                          │
      │                                                  │
      └──→ T11 (MCP async) ──→ T4                        │
                                                         │
T12 (raw LLM call) ──→ T5                                │
                                                         │
T7 (progress) ← T5, T6                                  │
T8 (planning tool) ← T5                                 │
T9 (sub-agent subprocess) ← T4                           │
T10 (budget enforcement) ← T5                            │
T13 (info/docs) ← T1                                    │
T14 (tests) ← all                                       │
T15 (external observability) ← T7                        │
```

### Recommended implementation phases

**Phase A — Foundation (T1, T2, T3, T12):**
Config, module skeleton, async eval, raw LLM call variant. No behavior change yet — pure infrastructure.

**Phase B — Core loop (T5, T6, T10):**
Iterative loop replaces recursion. Turn budget enforced. Callers refactored. At this point the loop works identically to before but is iterative with a budget.

**Phase C — Parallelism (T4, T11):**
Parallel tool execution. MCP async cleanup. The biggest performance improvement.

**Phase D — Observability (T7, T15):**
Progress events, trace rendering, and external observability (OSC titles, status file, notifications). Quality-of-life improvement and external tool compatibility.

**Phase E — Intelligence (T8, T9):**
Planning tool and sub-agent subprocess delegation. The composability features.

**Phase F — Polish (T13, T14):**
Documentation, info display, full test suite.

### Estimated scope

| Phase | Lines (approx) | Risk |
|-------|----------------|------|
| A | ~150 | Low — additive, no behavior change |
| B | ~250 | Medium — refactors two critical paths, but behavior should be identical |
| C | ~150 | Medium — concurrency correctness, but well-bounded by semaphore |
| D | ~200 | Low — additive, fire-and-forget channel + simple I/O |
| E | ~150 | Low — subprocess spawn is simpler than in-process recursion |
| F | ~100 | Low — tests and docs |
| **Total** | **~1000** | |

Total: approximately 1000 lines of new/modified code, comparable to the MCP bridge (1272 lines).
