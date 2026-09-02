# Coverage Re-Measurement — Backlog #5 (2026-09-02)

**Purpose:** Quantify the coverage impact of the backlog #5 test-hardening pass
(+17 tests on `feat/test-suite-hardening`) and establish a reproducible,
first-party-only methodology for future coverage runs.

---

## 1. Headline Result

The backlog #5 test-hardening pass raised `cargo test` coverage of the primary
target file `src/agent_loop.rs` by **+17.9 line points** (46.8% → 64.7%) and
**+16.9 function points** (50.0% → 66.9%), with a smaller lift on `src/mcp.rs`.
This is real, attributable improvement measured on identical methodology before
and after. The gain came in three waves: 13 pure-function/edge unit tests, 4
tests driving the `apply_output_routing` async dispatcher, then 4 tests on the
circuit-breaker and cost-budget helpers (extracted from `run()` for testability).

| File | Metric | Baseline (`main`, 319 tests) | Post-#5 (352 tests) | Δ |
|------|--------|:---:|:---:|:---:|
| `src/agent_loop.rs` | Line | 46.76% | **64.67%** | **+17.91** |
| `src/agent_loop.rs` | Function | 50.00% | **66.92%** | **+16.92** |
| `src/agent_loop.rs` | Region | 48.54% | **65.49%** | **+16.95** |
| `src/mcp.rs` | Line | 80.41% | **82.82%** | **+2.41** |
| `src/mcp.rs` | Function | 79.71% | **83.56%** | **+3.85** |
| `src/function.rs` | Line | 54.92% | 54.92% | 0.00 |
| **TOTAL** | Line | 53.81% | 54.82% | +1.01 |

Notes:
- `function.rs` is unchanged: the #5 tests exercised routing/cost/event/MCP
  paths, none of which added direct `function.rs` coverage. It remains a
  candidate for a future pass.
- The TOTAL line delta (+0.65) is small because the whole binary (~26k lines
  across clients, RAG, REPL, server) dilutes gains concentrated in one file —
  as expected for a targeted hardening pass.

---

## 2. Important: Why This Is NOT Comparable to the 72.9% Figure

The earlier report [`coverage-evaluation-2026-08-31.md`](file:///home/istari/projects/aichat/.kiro/docs/coverage-evaluation-2026-08-31.md)
measured `src/agent_loop.rs` at **72.9% line / 79.4% function**. That number was
produced by a **different methodology**: instrumenting the release binary and
running the full **live `run-demos.nu` E2E harness** (11 scenarios that invoke
real LLM providers, spawn sub-agent subprocesses, and drive the full runtime
loop).

This re-measurement uses the **`cargo test` unit-test suite only** — which
cannot reach the live runtime paths (actual streaming loops, real subprocess
spawns, provider I/O). So the absolute numbers here are *lower* by construction;
they are **not** a regression from 72.9%.

**The valid comparison is before-vs-after on the same methodology** (the table in
§1), which isolates exactly what the new tests added. To reproduce the 72.9%
style figure, run the E2E methodology (see §4, "E2E variant"), which is billed
and non-deterministic and therefore not part of routine measurement.

---

## 3. What the New Tests Covered

The +12-point `agent_loop.rs` gain comes from the 13 new unit tests hitting
functions that were previously **0% covered** by `cargo test`:

- `parse_cost_from_stderr` — cost extraction/edge parsing (was untested).
- `state_from_event` / `notification_for_event` — event→state/notification
  mapping across all 10 `AgentLoopEvent` variants (was untested).
- `detect_pipe_cycle` — multi-hop cycle branch (A→B→C→A).
- `apply_capping` — exact byte-boundary branch.
- `route_to_file` — missing-parent-directory creation branch.
- `expand_path_template` — timestamp + combined-variable branches.
- `eval_agent_tool_subprocess` — the depth-guard rejection branch.

The `mcp.rs` gain comes from the 4 new `parse_call_tool_result` edge tests
(missing content, error-without-text, unknown block types, image → data URI).

A second wave of 4 tests drives the **`apply_output_routing` async dispatcher**
directly (offline, no LLM), covering runtime routing branches the pure-helper
tests didn't reach: the pipe-cycle abort via the dispatcher (`pipe_cycle_error`),
file-destination dispatch, empty-target pipe fallback, and the default
context/capping path.

A third wave extracted two decision helpers out of the `run()` loop for
testability and unit-tested them directly: `update_circuit_breaker` (failure
counting, trip-after-3, success reset, independent per-tool tracking) and
`cost_budget_exceeded` (under/over/equal/zero-unlimited/negative-unlimited). This
was a minimal, behavior-preserving extract-method refactor (full suite stayed
green), not a rearchitecture. It added the final +2.2-line-point lift.

FR-4 (sub-agent crash isolation) is covered separately by **Demo 12** in
`scripts/run-demos.nu` (deterministic/offline), not by unit tests — see the
test-suite-hardening spec.

### Remaining uncovered paths in `agent_loop.rs` (honest limitations)

The circuit-breaker and cost-budget *decision logic* is now unit-tested (via the
extracted helpers). What remains uncovered is the surrounding `run()`
**orchestration** — the turn loop, streaming, event emission, tripped-call
short-circuit dispatch, and nested sub-agent recursion — all gated behind a live
`call_llm_raw` and therefore **not reachable by unit tests without a mock-client
seam**. Those paths are exercised by the live E2E harness (`run-demos.nu`) but
not by `cargo test`. Closing them deterministically would require introducing a
mock `Client` so `run()` can iterate turns without a provider — a larger,
separate piece of work, deliberately out of scope for this pass.

---

## 4. Reproducible Methodology (first-party tooling only)

Uses only `llvm-tools-preview` (a first-party rustup component). No third-party
crates (`cargo-llvm-cov`, `grcov`) are required.

### Prerequisites (one-time)

```bash
rustup component add llvm-tools-preview
```

The LLVM binaries then live under the toolchain sysroot:

```
$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin/
  ├── llvm-profdata
  └── llvm-cov
```

### Measure (per branch / commit)

```bash
# 1. Build the instrumented test binary (no-run so we can invoke it ourselves).
#    NOTE: first instrumented build recompiles the whole dep tree (10-15 min).
#    Subsequent builds are incremental (~30s-2min) once the cache is warm.
RUSTFLAGS="-C instrument-coverage" cargo test --bin aichat --no-run

# 2. Locate the freshly built test binary:
#      target/debug/deps/aichat-<hash>   (the extension-less executable)

# 3. Run it, emitting per-process profraw data to a scratch dir:
mkdir -p /tmp/cov
LLVM_PROFILE_FILE="/tmp/cov/aichat-%p-%m.profraw" \
  target/debug/deps/aichat-<hash>

# 4. Merge raw profiles:
BINDIR="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin"
"$BINDIR/llvm-profdata" merge -sparse /tmp/cov/aichat-*.profraw -o /tmp/cov/aichat.profdata

# 5. Report (exclude deps/stdlib so numbers reflect our code only):
"$BINDIR/llvm-cov" report target/debug/deps/aichat-<hash> \
  --instr-profile=/tmp/cov/aichat.profdata \
  --ignore-filename-regex='(/.cargo/|/rustc/|library/std)'

# For line-level detail on a single file, swap `report` for `show`:
"$BINDIR/llvm-cov" show target/debug/deps/aichat-<hash> \
  --instr-profile=/tmp/cov/aichat.profdata \
  --sources src/agent_loop.rs --show-line-counts-or-regions
```

### Gotchas

- **Full rebuild on flag change.** Toggling `RUSTFLAGS` invalidates the cargo
  cache, forcing a full instrumented rebuild the first time. Budget 10-15 min,
  or run the build detached and poll. Warm rebuilds are fast.
- **Binary crate, no `--lib`.** `aichat` has no library target; use
  `--bin aichat`. Tests live in the bin test executable.
- **Find the binary by recency**, not by a fixed hash (the hash changes with
  code). Pick the newest extension-less `target/debug/deps/aichat-*`.
- **Before/after comparisons must use the same methodology.** Do not compare a
  `cargo test` number against the live-harness 72.9% figure.

### E2E variant (matches the original 72.9% methodology — billed)

Same instrumentation, but instead of the unit-test binary, build the release
binary with `-C instrument-coverage` and run `nu scripts/run-demos.nu` against
live providers, then merge all per-process `.profraw` (parent + sub-agents) and
report. This exercises the runtime loop paths but requires API keys, incurs
cost, and is non-deterministic. Reserved for deliberate deep evaluations.

---

## 5. Artifacts

- Baseline (`main`): `llvm-cov report` output captured during this run.
- Post-#5 (`feat/test-suite-hardening`): captured during this run.
- Both used `--ignore-filename-regex='(/.cargo/|/rustc/|library/std)'` on the
  merged profile from the instrumented unit-test binary.

Coverage artifacts (`*.profraw`, `*.profdata`) are scratch/ephemeral and are not
committed.
