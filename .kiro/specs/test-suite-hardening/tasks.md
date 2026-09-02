# Test Suite Expansion & Code Coverage Hardening — Tasks

Branch: `feat/test-suite-hardening` (off `main` @ `64ebd7d`)

## Phase 1 — Output Routing Edge Cases (FR-1)

- [ ] 1.1 Add `pipe_cycle_detection_catches_multi_hop_cycle` (A→B→C→A) — extend the existing config-construction helper. (FR-1.1)
- [ ] 1.2 Add `pipe_cycle_detection_allows_multi_hop_linear_chain` (A→B→C). (FR-1.2)
- [ ] 1.3 Add `capping_passes_result_at_exact_limit_boundary` and `capping_caps_result_one_byte_over_limit`. (FR-1.3)
- [ ] 1.4 Add `file_routing_creates_missing_parent_directories` using an isolated temp dir + cleanup. (FR-1.4)
- [ ] 1.5 Add `template_expansion_substitutes_numeric_timestamp` and `template_expansion_resolves_all_variables_combined`. (FR-1.5)
- [ ] 1.6 `cargo test` — Phase 1 green.

## Phase 2 — Budget, Cost & Observability State (FR-2)

- [ ] 2.1 Add `parse_cost_extracts_dollar_amount_from_stderr`. (FR-2.1)
- [ ] 2.2 Add `parse_cost_returns_zero_when_marker_absent` and `parse_cost_tolerates_malformed_amount`. (FR-2.2)
- [ ] 2.3 Add `state_from_event_maps_all_variants` (all 10 `AgentLoopEvent` variants). (FR-2.3)
- [ ] 2.4 Add `notification_for_event_fires_only_on_terminal_events`. (FR-2.4)
- [ ] 2.5 `cargo test` — Phase 2 green.

## Phase 3 — Sub-Agent Depth Boundary (FR-3)

- [ ] 3.1 Add `agent_depth_guard_rejects_at_exact_max` (depth == max → reject, no spawn). (FR-3.1)
- [ ] 3.2 `cargo test` — Phase 3 green.

## Phase 4 — Stretch (FR-4, FR-5) — only if hermetic

- [ ] 4.1 (stretch) `subagent_nonzero_exit_yields_agent_error` — only if a failing invocation runs without providers/network. Otherwise leave a documented `// NOTE:` gap. (FR-4.1)
- [ ] 4.2 (stretch) `mcp_malformed_error_envelope_is_reported` — pure-parse only, extend `mcp::tests`. (FR-5.1)

## Phase 5 — Verify & Land

- [ ] 5.1 Full `cargo test` — all green, record new total count.
- [ ] 5.2 (optional) Re-run LLVM coverage on `agent_loop.rs`; update `.kiro/docs/coverage-evaluation-*` with the delta. Gated on time/cost — may defer.
- [ ] 5.3 Update `.kiro/docs/progress.md` (test count; mark backlog #5 status) and `backlog.md` #5 status.
- [ ] 5.4 Commit on `feat/test-suite-hardening` with a clear message referencing the covered FRs.

## Notes

- Use debug `cargo test` for iteration (release profile compile timed out at 400s in this environment).
- No production behavior changes. If a test surfaces a real bug, land the fix as a separate, clearly-labeled commit.
- Keep all new tests deterministic and hermetic (no network, no live providers, no tty, isolated temp paths).
