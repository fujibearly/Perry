# Test Suite Expansion & Code Coverage Hardening — Requirements

## Summary

Backlog item #5. Systematically add targeted tests to close the untested edge paths identified by the 2026-08-31 dynamic coverage evaluation, strengthening the foundation before further feature work. The focus is the fork's runtime-critical machinery: output routing, budget/cost tracking, sub-agent boundaries, and observability state mapping. No production code changes — this is a test-only hardening pass (with narrowly-scoped test hooks only where a pure function is currently unreachable from tests).

## Context

- The coverage report (`.kiro/docs/coverage-evaluation-2026-08-31.md`) measured the core agent loop at **72.9% line / 79.4% function** and flagged four expansion targets in §4:
  1. Output-routing cycle detection (recursive pipe aborts).
  2. Sub-agent crash isolation and non-zero exit codes.
  3. Strict cost budget (`max_cost`) mid-turn cancellation.
  4. Deeply-nested sub-agent depth boundary enforcement (`max_agent_depth`).
- Lower-covered subsystems (§2): `function.rs` (37%), `config/agent.rs` (39%), `client/stream.rs` (32%), `config/role.rs` (56%).
- The existing suite (327 tests) already covers the happy paths and several edges: capping (small/large/zero/hints), file routing (write + invalid-path fallback), template expansion, pipe cycle (self-reference + linear chain), config defaults/overrides, the `_plan` tool, progress snapshots, and depth-check env parsing.
- The remaining gaps fall into two tiers by test cost, which this spec treats differently (see Design Philosophy).

## Design Philosophy

1. **No production behavior change.** This is a hardening pass. Tests must exercise existing behavior; if a test reveals a real bug, fix it in a separate, clearly-labeled change rather than folding it silently into a test commit.
2. **Prefer pure-function unit tests.** The highest-value, lowest-risk, fastest coverage wins are deterministic tests of pure/near-pure functions (`detect_pipe_cycle`, `parse_cost_from_stderr`, `state_from_event`, `apply_capping`, `route_to_file`, `expand_path_template`). These run in milliseconds and never touch the network or spawn processes.
3. **Isolate filesystem tests.** Tests that write files must use a unique temp directory and clean up, never touching the real `/tmp/aichat-*` namespace used by running instances or the developer's home.
4. **Be honest about integration-tier gaps.** Real sub-agent crash capture (spawns the actual binary) and MCP handshake timeouts (spawns/mocks a server) are integration-level and slower/flakier. They are specified here but scoped as **stretch** — implemented only if they can be made deterministic and hermetic. If not, they remain documented gaps rather than flaky tests.
5. **Deterministic only.** No test may depend on wall-clock timing races, network access, live LLM providers, or an interactive terminal. Time-sensitive assertions (e.g. `expand_path_template` timestamp) assert structural properties, not exact values.

## Functional Requirements

### FR-1: Output-Routing Edge Cases (coverage §4.1)

- FR-1.1: A multi-hop pipe cycle (A→B→C→A) MUST be detected by `detect_pipe_cycle` and rejected. (Existing tests only cover self-reference A→A and a linear chain.)
- FR-1.2: A linear multi-hop chain (A→B→C, no cycle) MUST be accepted by `detect_pipe_cycle`.
- FR-1.3: `apply_capping` MUST pass a result whose serialized length is exactly `limit` through unchanged, and MUST cap a result of length `limit + 1`. (Exact byte boundary.)
- FR-1.4: `route_to_file` MUST create missing parent directories when the target path points into a not-yet-existing nested directory, and write the content there (FR-2.4 of the routing spec).
- FR-1.5: `expand_path_template` MUST substitute `{{timestamp}}` with a numeric value and MUST correctly expand a template combining multiple variables (`{{name}}`, `{{id}}`, `{{ext}}`, `{{timestamp}}`).

### FR-2: Budget, Cost & Observability State (coverage §4.3, stream.rs cost area)

- FR-2.1: `parse_cost_from_stderr` MUST extract the float following `Estimated cost: $` from a representative sub-agent stderr line.
- FR-2.2: `parse_cost_from_stderr` MUST return `0.0` when the marker is absent, and MUST tolerate malformed/non-numeric cost tokens without panicking (returns `0.0`).
- FR-2.3: `state_from_event` MUST map `BudgetExhausted` → `"budget_exhausted"`, `CostExhausted` → `"cost_exhausted"`, `LoopComplete` → `"done"`, and any working event (`ToolStart`, `TurnStart`, `PlanReceived`, `BudgetWarning`, …) → `"working"`.
- FR-2.4: `notification_for_event` MUST return a notification tuple for `LoopComplete`, `BudgetExhausted`, and `CostExhausted`, and `None` for non-terminal events.

### FR-3: Sub-Agent Depth Boundary (coverage §4.4)

- FR-3.1: The depth guard in `eval_agent_tool_subprocess` MUST reject a call when `AICHAT_AGENT_DEPTH >= max_agent_depth` (boundary: equal is rejected). This is verified at the depth-check level without actually spawning a subprocess, extending the existing `agent_depth_check_respects_env_var` coverage to the exact boundary condition.

### FR-4: Sub-Agent Crash Isolation (coverage §4.2) — COVERED VIA E2E

- FR-4.1: A sub-agent subprocess that exits non-zero MUST surface a captured, readable error rather than crashing the parent (the parent's `eval_agent_tool_subprocess` wraps the child's non-zero exit + stderr as an `agent_error`). Because that function spawns `std::env::current_exe()` — the *test* binary under `cargo test`, not `aichat` — this is not hermetically testable as a unit test. It is instead covered by **Demo 12 (Sub-Agent Crash Isolation)** in `scripts/run-demos.nu`: a deterministic, offline check that runs the real binary with an unknown agent name and asserts (a) non-zero exit, (b) a captured/readable error, (c) no panic. No live providers or network required.

### FR-5: MCP Bridge Error Paths — STRETCH

- FR-5.1: When feasible, MCP handshake timeout and malformed JSON-RPC error responses MUST be covered by unit tests against the parsing/decoding functions (not a live server). Existing `mcp::tests` already cover result parsing (text/json/error/empty) and env expansion; new tests extend only the error-decode edges. Implemented only if deterministic.

## Non-Functional Requirements

- NFR-1: All existing 327 tests MUST continue to pass. Zero regressions.
- NFR-2: New tests MUST run under the default `cargo test` profile without network access, live API keys, spawned LLM calls, or a tty.
- NFR-3: New tests MUST be deterministic (no timing races; no reliance on ordering of parallel execution beyond what the code guarantees).
- NFR-4: Filesystem-touching tests MUST use unique temp paths (PID/nonce or a temp-dir helper) and clean up after themselves.
- NFR-5: Test names MUST be descriptive and state the invariant under test (matching the existing `subject_does_expected_thing` convention).

## Out of Scope

- Refactoring production code for testability beyond making an already-pure function reachable (no architectural changes).
- Raising coverage of rendering (`render/`), REPL, or HTTP-server modules — not identified as runtime-critical gaps by the evaluation.
- Re-running the full LLVM coverage instrumentation as part of CI (a manual re-measure is a follow-up task, not a requirement here).
- Fixing the separately-noted `$0.000000` cost-estimator display bug (tracked independently; not a test concern).
- Any change to the live `~/clones/llm-functions` directory.
