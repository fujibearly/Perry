# Session 28: Backlog #19 Implemented & Verified — Autonomy Ladder (`--autonomy <readonly|consult|reversible>`)

**Period:** `2026-09-17`  
**Branch:** `feat/autonomy-ladder`  
**Handoff Document:** `.kiro/docs/session-summary-2026-09-17-session28.md`  
**Consolidated Index Entry:** `SESSION_SUMMARY.md` (#28)

---

## 1. Executive Summary

Backlog Item #19 (**Autonomy Ladder**) has been fully implemented, integrated, and verified across all execution paths.

The Autonomy Ladder establishes domain-agnostic, honest operator postures (`readonly`, `consult`, `reversible`) that act as ergonomic macro presets coordinating the underlying 2D safety matrix (Capability Mask vs. Authority Ceiling) established in Backlog #6, without conflating the axes or proliferating environment variables ("environment variable soup").

---

## 2. Core Architectural Principles & Decisions

1. **Preservation of the 2D Safety Matrix:**
   - **Mask Axis (Gate 1):** Sets what tools an agent may legally invoke (`readonly` vs. `mutating`).
   - **Authority Ceiling Axis (Gate 2):** Sets the maximum blast radius an agent may actuate autonomously (`safe` vs. `reversible` vs. `disruptive` vs. `destructive`).
   - The `--autonomy` flag sets a macro posture at the root orchestrator level without flattening or destroying the distinction between capability masks and authority ceilings.

2. **Domain-Agnostic Nomenclature:**
   - Rejected opaque codes (`a0`, `a1`, `a2`) as primary flags; preserved them as loose parsing aliases.
   - Rejected misleading terminology (`autopilot`, which overpromises infinite autonomy).
   - Rejected overloaded terminology (`delegate`, which collides with sub-agent delegation).
   - Selected:
     - `readonly` (Observer / A0): Read-only capability mask + `safe` ceiling.
     - `consult` (Copilot / A1): Unmasked capability + `safe` ceiling + clamped Option B autonomous bypass.
     - `reversible` (Safe Autonomous / A2): Unmasked capability + `reversible` ceiling + active Option B preflight remediation.

3. **No "Environment Variable Soup" & Strict Subagent Sandboxing:**
   - `--autonomy` is strictly an operator/orchestrator macro.
   - Child sub-agents never receive an `AICHAT_AUTONOMY` environment variable.
   - Child sub-agents are strictly provisioned via canonical `DelegatedPermissions` (`AICHAT_CAPABILITY_MASK` and `AICHAT_AUTHORITY_CEILING`) and cannot escalate past their parent's posture or ceiling.

4. **Evaluator-First Unified Human Consultation Funnel:**
   - Eliminated the Gate 2 / Gate 3 double-prompt trap.
   - Rather than prompting the user at Gate 2 and then prompting again after Gate 3 `%assess-risk%`, Gate 3 runs *first* whenever Gate 2 trips `authority_exceeded`.
   - A single unified prompt is presented to the operator containing both the authority delta and the evaluator's risk analysis.
   - Avoided dangerous "pre-approval" of Gate 3 based on Gate 2 approval.

5. **Option B Autonomous Reversibility Gating:**
   - Gated Option B preflight auto-remediation on `level.permits_autonomous_reversibility()`.
   - In `consult` mode, preflight step-down cannot discount mutations below `Reversible` into autonomous execution without operator authorization.

---

## 3. As-Built Implementation Details

1. **`src/safety.rs`:**
   - Added `enum AutonomyLevel { ReadOnly, Consult, Reversible }` with `Serialize`, `Deserialize`, `as_str()`, and `from_str_loose()`.
   - Added posture mappings: `capability_mask()`, `authority_ceiling()`, and `permits_autonomous_reversibility()`.
   - Added 4 unit tests covering parsing, aliases, serde round-trip, and posture mappings.

2. **`src/config/mod.rs` & `src/cli.rs`:**
   - Added `pub autonomy: Option<AutonomyLevel>` to `SafetyConfig` (default `None`).
   - Wired `AICHAT_AUTONOMY` and `AICHAT_SAFETY_AUTONOMY` env vars into `load_envs`.
   - Added `--autonomy <POSTURE>` CLI flag with top-precedence configuration override.
   - Added unit test for YAML config deserialization.

3. **`src/agent_loop.rs`:**
   - Root macro expansion at entry to `run()` / `run_agent()`: sets `AICHAT_CAPABILITY_MASK=readonly` if `ReadOnly` and overrides root authority ceiling baseline.
   - Startup trace banner: `[safety posture: <level> — capability_mask=..., authority_ceiling=...]`.
   - Precedence order in `current_authority_ceiling()`: child `AICHAT_AUTHORITY_CEILING` > explicit `AICHAT_SAFETY_DEFAULT_CEILING` > `safety.autonomy` baseline > `safety.default_ceiling`.
   - Gated Option B preflight step-down on `permits_autonomous_reversibility()`.
   - Refactored `eval_single_tool` into the Evaluator-First Unified Human Consultation Funnel.
   - Added unit tests:
     - `test_autonomy_posture_precedence_and_ceilings`
     - `test_autonomy_posture_consult_blocks_option_b_bypass`
     - `test_autonomy_readonly_blocks_mutating_at_gate_1`
     - `test_autonomy_subagent_delegation_isolation`

4. **`scripts/run-demos.nu`:**
   - Added comprehensive dedicated **Demo 24** exercising all three postures (`readonly`, `reversible`, `consult`) live against real tools.
   - Migrated **Demo 8** (`fetch_and_summarize`) to `--autonomy readonly`.
   - Migrated **Demo 10b** (`read_pdf` page selection) to `--autonomy readonly`.
   - Migrated **Demo 18** (`fs_write` Option B preflight remediation) to `--autonomy reversible`, retiring the low-level `AICHAT_SAFETY_DEFAULT_CEILING: "reversible"` environment variable.

---

## 4. Verification & Testing

- **Unit & Integration Suite:**
  - `cargo test`: **558 passed, 0 failed, 0 ignored**
  - All 8 autonomy-specific tests passing.
- **Linting & Code Quality:**
  - `cargo clippy -- -D warnings`: **Clean (0 warnings, 0 errors)**
- **Live End-to-End Demo Invocations:**
  - `nu scripts/run-demos.nu -t 8`: PASSED (ReadOnly pipe routing)
  - `nu scripts/run-demos.nu -t 10b`: PASSED (ReadOnly PDF extraction)
  - `nu scripts/run-demos.nu -t 18`: PASSED (Reversible Option B pre-flight remediation)
  - `nu scripts/run-demos.nu -t 24`: PASSED (3-posture Autonomy Ladder validation)
