# Test Suite Expansion & Code Coverage Hardening — Tasks

Branch: `feat/test-suite-hardening` (off `main` @ `64ebd7d`)

## Phase 1 — Output Routing Edge Cases (FR-1)

- [x] 1.1 Add `pipe_cycle_detection_catches_multi_hop_cycle` (A→B→C→A) — extend the existing config-construction helper. (FR-1.1)
- [x] 1.2 Add `pipe_cycle_detection_allows_multi_hop_linear_chain` (A→B→C). (FR-1.2)
- [x] 1.3 Add `capping_passes_result_at_exact_limit_boundary` and `capping_caps_result_one_byte_over_limit`. (FR-1.3)
- [x] 1.4 Add `file_routing_creates_missing_parent_directories` using an isolated temp dir + cleanup. (FR-1.4)
- [x] 1.5 Add `template_expansion_substitutes_numeric_timestamp` and `template_expansion_resolves_all_variables_combined`. (FR-1.5)
- [x] 1.6 `cargo test` — Phase 1 green.

## Phase 2 — Budget, Cost & Observability State (FR-2)

- [x] 2.1 Add `parse_cost_extracts_dollar_amount_from_stderr`. (FR-2.1)
- [x] 2.2 Add `parse_cost_returns_zero_when_marker_absent` and `parse_cost_tolerates_malformed_amount`. (FR-2.2)
- [x] 2.3 Add `state_from_event_maps_all_variants` (all 10 `AgentLoopEvent` variants). (FR-2.3)
- [x] 2.4 Add `notification_for_event_fires_only_on_terminal_events`. (FR-2.4)
- [x] 2.5 `cargo test` — Phase 2 green.

## Phase 3 — Sub-Agent Depth Boundary (FR-3)

- [x] 3.1 Add `agent_depth_guard_rejects_at_exact_max` (depth == max → reject, no spawn). Implemented as a `#[tokio::test]` using `max_agent_depth: 0` so the guard fires at default depth without touching the process-global `AICHAT_AGENT_DEPTH` env var (race-free under parallel tests). (FR-3.1)
- [x] 3.2 `cargo test` — Phase 3 green.

## Phase 4 — Stretch (FR-4, FR-5) — only if hermetic

- [x] 4.1 (stretch) `subagent_nonzero_exit_yields_agent_error` — **DEFERRED as documented gap.** Not hermetically achievable at the unit level: `eval_agent_tool_subprocess` spawns `std::env::current_exe()`, which under `cargo test` is the *test* binary, not the `aichat` binary. Spawning it with `--agent` would exercise the test harness's arg handling, not aichat's sub-agent path, producing a misleading test. The depth-guard boundary (FR-3.1) — the part reachable before any spawn — IS covered by `agent_depth_guard_rejects_at_exact_max`. True crash-capture belongs in an E2E test that runs the real release binary (the existing `run-demos.nu` harness is the right home), not a unit test. (FR-4.1)
- [x] 4.2 (stretch) `mcp_malformed_error_envelope_is_reported` — DONE. Added 4 pure-parse tests to `mcp::tests`: missing `content` field, error-without-text, unknown block types skipped, image block → data URI. (FR-5.1)

## Phase 5 — Verify & Land

- [x] 5.1 Full `cargo test` — all green (344 pass, 0 fail: 336 unit + 5 catalog-override + 3 integration; was 327).
- [ ] 5.2 (optional) Re-run LLVM coverage on `agent_loop.rs`; update `.kiro/docs/coverage-evaluation-*` with the delta. **DEFERRED** — gated on the instrumented build + live billed harness; improvement argued qualitatively instead (17 tests hitting previously-0%-covered functions).
- [x] 5.3 Update `.kiro/docs/progress.md` (test count 327→344; #5 status In Progress) and `backlog.md` #5 status.
- [x] 5.4 Commit on `feat/test-suite-hardening` (`b6505f3`) with a message referencing the covered FRs. (Spec committed earlier at `3cb4e71`.)

## Notes

- Use debug `cargo test` for iteration (release profile compile timed out at 400s in this environment).
- No production behavior changes. If a test surfaces a real bug, land the fix as a separate, clearly-labeled commit.
- Keep all new tests deterministic and hermetic (no network, no live providers, no tty, isolated temp paths).
