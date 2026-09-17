# Session Summary 27: Structured Plan & Skills Merge, Codename Perry, 3-Tier Glossary & Semantic Types

**Date:** 2026-09-16  
**Branch:** `main` (fast-forward merged from `feat/glossary-and-semantic-types` at commit `bc235fe`)  
**Base Commit:** `ddd8a1c` (Merge of `feat/structured-plan-and-skills` #15 & #17)  
**Commits Landed in Session 27:**
- [`ddd8a1c`](file:///home/istari/projects/aichat) `fix(clippy): resolve clippy warnings for map_or, needless_match, and too_many_arguments`
- [`816987e`](file:///home/istari/projects/aichat) `docs: add comprehensive glossary and steering updates`
- [`e5f242a`](file:///home/istari/projects/aichat) `refactor: add ImpactTier newtype with ergonomic conversions and round-trip tests`
- [`32f28d0`](file:///home/istari/projects/aichat) `refactor: add permits_impact() helper to AuthorityCeiling`
- [`f08fb95`](file:///home/istari/projects/aichat) `refactor: migrate agent loop Should Gate to use ImpactTier`
- [`bc235fe`](file:///home/istari/projects/aichat) `docs: add glossary migration roadmap for future refactoring`

**Test Suite Status:** 530 passing tests (`cargo test --bin aichat`, 0 failed); `cargo clippy --bin aichat -- -D warnings` clean; `cargo doc --no-deps` clean; Live safety demos 16, 17, 18, 19, 20 passing.  
**Key Documents Created/Updated:**
- [`.kiro/docs/glossary.md`](file:///home/istari/projects/aichat/.kiro/docs/glossary.md) (Canonical single source of truth for Project Perry terminology)
- [`.kiro/docs/GLOSSARY_MIGRATION_ROADMAP.md`](file:///home/istari/projects/aichat/.kiro/docs/GLOSSARY_MIGRATION_ROADMAP.md) (Tier 3 migration roadmap for future refactorings)
- [`.kiro/steering/project-context.md`](file:///home/istari/projects/aichat/.kiro/steering/project-context.md) (Project codename Perry + mandatory canonical glossary mandate)
- [`.kiro/architecture.md`](file:///home/istari/projects/aichat/.kiro/architecture.md) & [`.kiro/docs/roadmap.md`](file:///home/istari/projects/aichat/.kiro/docs/roadmap.md) (Linked to canonical glossary)

---

## 1. Executive Summary

Session 27 completed three major project milestones:
1. **Merge of #15 (Structured Plan) & #17 (Skills) to `main` (`ddd8a1c`):**
   - Cleaned up 4 clippy warnings (`is_some_and`, argument count attributes, unnecessary matching in skill tool call).
   - Verified clean test suite (529 tests) and merged `feat/structured-plan-and-skills` into `main`.
2. **Project Codename Establishment:**
   - Grounded codename **Perry (Agent P)** across `.kiro/steering/project-context.md`, `architecture.md`, and `roadmap.md`.
3. **Execution & Merge of 3-Tier Glossary & Semantic Types Refactoring (5 Commits, `816987e`..`bc235fe`):**
   - **Tier 1 (Immediate / Grounded):** Published canonical [`.kiro/docs/glossary.md`](file:///home/istari/projects/aichat/.kiro/docs/glossary.md) covering 7 core domains, 8 historical ambiguities, and 4 missing foundational terms (`Should Gate`, `Proven Reversibility`, `Downward Permit Prohibition`, and `Dual-Arm Parser`). Added code docstrings in `src/function.rs`, `src/safety.rs`, and `src/agent_loop/plan.rs`.
   - **Tier 2 (Surgical / Non-breaking):**
     - Defined transparent newtype [`ImpactTier(pub BlastRadius)`](file:///home/istari/projects/aichat/src/function.rs) with `#[serde(transparent)]`, preserving existing serialization contracts (`"safe"`, `"reversible"`, `"disruptive"`, `"destructive"`, `"catastrophic"`).
     - Added [`AuthorityCeiling::permits_impact(&self, impact: ImpactTier) -> bool`](file:///home/istari/projects/aichat/src/safety.rs) to cleanly express the relationship between agent authority and action impact.
     - Migrated the Should Gate hot path in `src/agent_loop.rs` (`eval_single_tool` and Option B pre-flight remediation) to query `ceiling.permits_impact(ImpactTier(t))`.
   - **Tier 3 (Deferred / Roadmap):** Published [`.kiro/docs/GLOSSARY_MIGRATION_ROADMAP.md`](file:///home/istari/projects/aichat/.kiro/docs/GLOSSARY_MIGRATION_ROADMAP.md) detailing the evolutionary roadmap for future renames (e.g., `SafetyClass` $\to$ `CapabilityMask` under Backlog #6a/polish, and `BlastRadius` internal naming).
   - Fast-forward merged `feat/glossary-and-semantic-types` into `main`.

---

## 2. Technical Deliverables & Architecture Details

### A. Semantic Disambiguation (`glossary.md`)
The canonical glossary establishes exact definitions across 7 domains:
1. **Safety & Containment:** Disambiguates `BlastRadius` (intrinsic danger scale), `AuthorityCeiling` (agent's maximum permitted impact), `ImpactTier` (newtype for tool risk), and `SafetyClass`/`ToolMode` (process capability filter).
2. **Execution & Orchestration:** Formalizes `Dual-Arm Parser` resilience in `PlanPayload::parse_flexible` and the ReAct execution lifecycle.
3. **Multi-Agent Coordination & Escalation:** Clarifies `Downward Permit Prohibition` (supervisors cannot issue execution waivers; child must unwind and report `authority_exceeded`), `The Should Gate`, and mTLS channel binding.
4. **Skills & Runbook Management:** Defines 3-tier precedence (`workspace` > `global` > `builtin`), `WorkspaceTainted` status, and step-scoped taint clearance.
5. **Data Flow & Routing:** Auto-capping (16KB), file output, and acyclic pipe chains.
6. **Observability & Traceability:** 6-column guide rails (`│     `), FIFO dialog event ordering, British humour petnames, and ANSI soft-wrapping.
7. **Storage & Durability:** 0600 rollback journals, staging conventions, and WAL resumption.

### B. Safe Newtype Pattern (`src/function.rs` & `src/safety.rs`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ImpactTier(pub BlastRadius);

impl ImpactTier {
    pub fn as_blast_radius(&self) -> BlastRadius {
        self.0
    }
}
```
- Round-trip tests verify that serde JSON output matches raw `BlastRadius` strings (`"safe"`, `"disruptive"`).
- `AuthorityCeiling::permits()` delegates to `permits_impact()`, preserving backwards compatibility with `RequiredAuthority::Tier(t)`.
- `FunctionDeclaration::impact_tier(&self) -> Option<ImpactTier>` provides a direct accessor for tool declarations.

### C. Hot-Path Gating Migration (`src/agent_loop.rs`)
- In `eval_single_tool`:
  ```rust
  let permitted = match required {
      RequiredAuthority::Tier(t) => ceiling.permits_impact(crate::function::ImpactTier(t)),
      RequiredAuthority::Human => false,
  };
  ```
- In Option B preflight remediation:
  ```rust
  let remediated_permitted = match remediated_required {
      RequiredAuthority::Tier(t) => ceiling.permits_impact(crate::function::ImpactTier(t)),
      RequiredAuthority::Human => false,
  };
  ```

---

## 3. Verification & Validation Summary

| Test Category | Suite / Command | Outcome |
| :--- | :--- | :--- |
| **Workspace Unit & Integration Tests** | `cargo test --bin aichat` | **530 passed, 0 failed, 0 ignored** |
| **Safety Tests** | `cargo test safety` | **All 44 safety tests passed** |
| **Agent Loop Tests** | `cargo test agent_loop` | **All 128 agent loop tests passed** |
| **Clippy Linter** | `cargo clippy --bin aichat -- -D warnings` | **Clean (0 warnings)** |
| **Rustdoc Intra-Doc Links** | `cargo doc --no-deps` | **Clean (0 warnings)** |
| **Demo 16** | `nu scripts/run-demos.nu -t 16` | **PASSED (Offline Escalation & 0600 Journal)** |
| **Demo 17** | `nu scripts/run-demos.nu -t 17` | **PASSED (Happy Path Autonomous Write)** |
| **Demo 18** | `nu scripts/run-demos.nu -t 18` | **PASSED (Option B Pre-flight Remediation)** |
| **Demo 19** | `nu scripts/run-demos.nu -t 19` | **PASSED (Authority Ceiling Fail-Closed)** |
| **Demo 20** | `nu scripts/run-demos.nu -t 20` | **PASSED (Hard Child Ceiling & Re-delegation)** |
| **Demo 22** | `nu scripts/run-demos.nu -t 22` | **PASSED (Progressive Runbook - Builtin Trusted)** |
| **Demo 23** | `nu scripts/run-demos.nu -t 23` | **PASSED (Workspace Skill Discovery & Provenance Taint)** |
| **Full Suite Unit Tests** | `cargo test --bin aichat agent_loop::tests -- --test-threads=1` | **124 passed, 0 failed in 0.48s (542+ total)** |
| **Full Suite Clippy** | `cargo clippy --bin aichat -- -D warnings` | **Clean (0 warnings)** |
| **Release Compilation** | `cargo build --release --bin aichat` | **Clean (Finished release [optimized])** |

---

## 4. Part B: Scoped Tools, Progressive Runbooks POC, Safety Gate Sequencing & Trace Observability

Following the glossary and semantic types merge, Session 27 implemented and verified critical engine enhancements across token efficiency, progressive runbook execution, causal safety gating, and trace observability:

### A. Scoped Tool Subsetting (`-r %functions:tool1,tool2%`)
- **Problem:** Declaring all 31 tools indiscriminately injected ~6,000 tokens of schema boilerplate into every turn.
- **Solution:** Added dynamic tool scoping syntax `-r %functions:<tool1>,<tool2>%` in `src/config/mod.rs` (`extract_role` and `select_functions`).
- **Impact:** Reduced schema overhead across 14 demos in `scripts/run-demos.nu` down to ~200-400 tokens per turn (~95% token savings).

### B. Progressive Disclosure Skills Proof-of-Concept (Demos 22 & 23)
- **Builtin Skill Fixture:** Created `assets/builtin-skills/sys_triage/SKILL.md`.
- **Classification:** Registered `read_skill` as `SafetyClass::Readonly`, `BlastRadius::Safe`, and `intrinsic_reversible: true`.
- **Implicit Catalog Injection:** Added `role.append_prompt` in `src/config/role.rs` so non-agent roles automatically receive the `### Available Skills` catalog and `read_skill` tool whenever eligible skills are present.
- **Demo 22 (Builtin, Trusted):** Verified end-to-end progressive disclosure via `read_skill`, running diagnostic inspection, writing report artifact, and outputting summary.
- **Demo 23 (Workspace, Untrusted Taint):** Verified workspace skill discovery (`.kiro/skills`), automatic `WorkspaceTainted` marking, active taint lifecycle (`untrusted_runbook: true` on active plan step), and heightened scrutiny in `%assess-risk%`.

### C. Causal Safety Gate Sequencing (`ALLOW` After Assessment)
- **Problem:** Static `ALLOW` comparisons were emitted before dynamic `%assess-risk%` evaluation, producing contradictory traces when `%assess-risk%` raised the tier.
- **Solution:** In `eval_single_tool`, static `ALLOW` events are deferred whenever `will_consult_risk_evaluator` is true. The authorization comparison `ALLOW <tool>: risk <tier> <= ceiling <tier>` is emitted strictly **after** the evaluator has verified the action.

### D. Real-Time Model & Token Count Attribution
- **Turn Start Trace Lines:** Added `model: Option<String>` and `tokens: Option<usize>` to `AgentLoopEvent::TurnStart`. Formats `{tok} tok` in `DarkGray` immediately before `@ <model>` in Yellow: `[<agent> <pid> (<petname>) 286 tok @ <model> [turn X/Y] starting]`.
- **Dialog Frames:** Added `tokens: Option<usize>` to `AgentLoopEvent::DialogBlock` and `format_dialog_block_with_model`. Headers display `{tok} tok` in `DarkGray` before `@ <model>` on prompt submissions (`📥`) and LLM responses (`📤`).
- **Upfront Computation:** Request token estimates are computed via `model.total_tokens(&msgs)` before network dispatch, providing real-time visibility into turn-by-turn context inflation.

### E. Evaluator Script & Command Pretty-Formatting in `--dialog`
- **Problem:** Evaluator dialog blocks displayed script implementations and arguments as dense JSON strings with escaped quotes (`\"`) and newlines (`\n`).
- **Solution:** Implemented `pretty_format_evaluator_context` and `format_evaluator_dialog_prompt` in `src/agent_loop.rs`. Unescapes tool source code, helper scripts, and commands into clean Markdown code blocks (````bash ... ````), while strictly preserving raw byte JSON on the wire to the model.

### F. Harness Ergonomics & Knowledge Base
- **Prompt Isolation:** In `scripts/run-demos.nu`, user prompts are visually separated from CLI flags/environment variables, rendered on their own line in highlighted `light_cyan` with blank spacing before and after.
- **Postmarked Knowledge Base:** Established [`lesssons-learned.md`](file:///home/istari/projects/aichat/lesssons-learned.md) with 14 postmarked architectural insights, failure mode diagnoses, and operational rules for future agents.

---

## 5. Next Milestone Candidates

1. **Backlog #8 (Dynamic Context Compaction):** Implement rolling micro-summarization (turns 1..N-3) and evict one-off skill runbook bodies once their active plan step completes.
2. **Backlog #7 (Session Resumption & WAL Journaling):** Implement `$XDG_RUNTIME_DIR/aichat-<session>.wal` append-only event stream and `--resume <session-id>` flag.
3. **Backlog #11 (Mock-Client Test Seam):** Provide a deterministic test seam for `call_llm_raw` to eliminate all network dependencies in unit tests.
