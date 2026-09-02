# Test Suite Expansion & Code Coverage Hardening — Design

## Approach

Add unit tests co-located in the existing `#[cfg(test)] mod tests` blocks of the modules under test (matching the fork's convention — `agent_loop.rs`, `function.rs`, and `mcp.rs` all keep tests inline). No new integration-test files unless a stretch item requires process spawning, in which case it goes under `tests/`.

Each target maps to a concrete, already-existing function seam identified during code inspection. The functions are pure or near-pure, so tests construct inputs directly and assert on returned `serde_json::Value` / `&str` / `Result`.

## Seam Map

| Requirement | Function under test | Location | Purity | Test technique |
|-------------|---------------------|----------|--------|----------------|
| FR-1.1/1.2 | `detect_pipe_cycle` | `agent_loop.rs:646` | Reads `GlobalConfig` (in-memory) | Build a `Config` with `mapping_tools`/routing that forms A→B→C→A vs A→B→C; assert `Err`/`Ok`. Reuse the existing test's config-construction pattern. |
| FR-1.3 | `apply_capping` | `agent_loop.rs:541` | Pure (writes temp file only when capping) | Feed a JSON string of length exactly `limit` (unchanged) and `limit+1` (capped → object with `preview`/`full_output_path`). Use a unique tool name so the temp path is isolated; remove it after. |
| FR-1.4 | `route_to_file` | `agent_loop.rs:585` | Pure + fs write | Point `OutputRouting.path` at `<tempdir>/nested/dir/out.txt`; assert `written_to` returned and file exists with correct content; assert the nested dirs were created. Clean up tempdir. |
| FR-1.5 | `expand_path_template` | `agent_loop.rs:671` | Pure (reads clock) | Assert `{{timestamp}}` expands to all-ASCII-digits; assert a combined template resolves each var and leaves no `{{` markers. |
| FR-2.1/2.2 | `parse_cost_from_stderr` | `agent_loop.rs:1182` | Pure | Feed representative lines: `"... Estimated cost: $0.0123 ..."` → `0.0123`; no marker → `0.0`; `"Estimated cost: $abc"` → `0.0` (no panic). |
| FR-2.3 | `state_from_event` | `agent_loop.rs:1207` | Pure | Construct each `AgentLoopEvent` variant; assert the mapped state string. |
| FR-2.4 | `notification_for_event` | `agent_loop.rs:1224` | Pure | Assert `Some((..))` for the three terminal events, `None` for a working event. |
| FR-3.1 | depth guard in `eval_agent_tool_subprocess` | `agent_loop.rs:382` | Reads env + config | The guard is the first thing the fn does. Rather than spawn, extract/verify via the existing env-var seam: set `AICHAT_AGENT_DEPTH == max_agent_depth` and assert the call returns the depth `Err` before any spawn. If the guard is not independently callable, assert through the public entry with a config whose `max_agent_depth` equals the env depth, confirming the bail message — without providers, the spawn is never reached because the guard fires first. |
| FR-4.1 (stretch) | subprocess error branch | `agent_loop.rs:382` | Spawns `current_exe()` | Only if a hermetic failing invocation exists (e.g. `--agent <nonexistent>` exits non-zero quickly without needing a provider). Assert `agent_error` shape. Skip if it requires network/keys. |
| FR-5.1 (stretch) | MCP error decode | `mcp.rs` | Pure parse | Extend existing `parse_call_tool_result_*` tests with a malformed/error JSON-RPC envelope. Only pure-parse paths; no server spawn. |

## Key Design Decisions

- **Inline unit tests, not new files.** Keeps the change consistent with the codebase and lets tests reach private functions (`detect_pipe_cycle`, `parse_cost_from_stderr`, etc. are module-private). A separate `tests/` integration file cannot see them.
- **Config construction reuse.** `detect_pipe_cycle` needs a `GlobalConfig` whose tool routing forms a chain. The existing `pipe_cycle_detection_*` tests already build such configs; the new multi-hop tests extend that exact helper pattern rather than inventing a new fixture.
- **Temp-file isolation.** `apply_capping` writes to `/tmp/aichat-tool-<name>-<pid>.out` and `route_to_file` writes to the template path. Tests use a unique tool name / a process-unique temp subdirectory and delete artifacts in the test body (the codebase has no `tempfile` dev-dependency yet; prefer `std::env::temp_dir().join(format!("aichat-test-{}-{}", pid, nonce))` and explicit cleanup to avoid adding a dependency for this pass).
- **Boundary framing.** "Exact boundary" tests are deliberate: `len == limit` (pass) vs `len == limit + 1` (cap), and `depth == max` (reject). These are the off-by-one seams most likely to regress silently.
- **Stretch items stay stretch.** FR-4/FR-5 are only landed if hermetic. The requirements explicitly permit leaving them as documented gaps; a flaky process/network test is worse than an honest TODO.

## Test Inventory (planned)

Target: ~12-16 new deterministic unit tests.

1. `pipe_cycle_detection_catches_multi_hop_cycle` (FR-1.1)
2. `pipe_cycle_detection_allows_multi_hop_linear_chain` (FR-1.2)
3. `capping_passes_result_at_exact_limit_boundary` (FR-1.3)
4. `capping_caps_result_one_byte_over_limit` (FR-1.3)
5. `file_routing_creates_missing_parent_directories` (FR-1.4)
6. `template_expansion_substitutes_numeric_timestamp` (FR-1.5)
7. `template_expansion_resolves_all_variables_combined` (FR-1.5)
8. `parse_cost_extracts_dollar_amount_from_stderr` (FR-2.1)
9. `parse_cost_returns_zero_when_marker_absent` (FR-2.2)
10. `parse_cost_tolerates_malformed_amount` (FR-2.2)
11. `state_from_event_maps_all_variants` (FR-2.3)
12. `notification_for_event_fires_only_on_terminal_events` (FR-2.4)
13. `agent_depth_guard_rejects_at_exact_max` (FR-3.1)
14. (stretch) `subagent_nonzero_exit_yields_agent_error` (FR-4.1)
15. (stretch) `mcp_malformed_error_envelope_is_reported` (FR-5.1)

## Verification

- Run `cargo test` (debug profile — faster compile than release) after each cluster of additions.
- Final full-suite run must show the new count with 0 failures.
- Optional follow-up (not required by this spec): re-run LLVM coverage instrumentation to quantify the delta on `agent_loop.rs` and update the coverage doc. Documented as a task, gated on time/cost.
