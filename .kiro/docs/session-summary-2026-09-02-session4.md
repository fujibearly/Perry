# Session Summary & Agent Handoff (Session 4: 2026-09-02)

**Period:** `2026-09-02` (distinct working session; same calendar day as Session 3 but separate work — spec authoring + first implementation increment of backlog #6).
**Repository:** `/home/istari/projects/aichat`
**Branch at handoff:** `feat/tool-safety-6a` @ `8994922` (2 commits ahead of `main`).
**Version:** `v0.31.0-fork.9`

---

## 1. Executive Summary

This session **designed backlog #6 (Tool Safety Modes & Actuation Governance) in depth and implemented its first increment (#6a)**. The original ~150-250-line binary capability mask grew — through an extended design conversation with the user — into a four-part **umbrella** (#6a–#6d) with a layered decision funnel. A full umbrella spec was written and committed, then increment **#6a (deterministic capability mask)** was implemented, tested, documented, and committed.

All work is local-only on the `feat/tool-safety-6a` branch (nothing pushed, per standing user preference).

---

## 2. Git State (verified at handoff)

- **Current branch:** `feat/tool-safety-6a`, HEAD `8994922`, branched off `main` @ `bef34a3`.
- **2 commits ahead of `main`:**
  1. `c28dfd5` — docs: add spec for backlog #6 — Tool Safety Modes & Actuation Governance (umbrella spec + folds #6a–#6d into backlog.md/progress.md)
  2. `8994922` — feat: backlog #6a — deterministic tool safety capability mask (6 files, +397/−16)
- **`main` is 140 commits ahead of `origin/main`. NOTHING PUSHED.** Local-only is deliberate; no remote backup.
- **Working tree clean** except the three long-standing intentionally-untracked files: `.kiro/docs/agent-loop-operation.mmd`, `AI.pdf`, `manual.pdf` (`manual.pdf` used by the demo harness). These were deliberately NOT staged.
- **Branch strategy (from the spec):** one branch per increment, each off the previous —
  `main → feat/tool-safety-6a → feat/tool-safety-6b → …6c → …6d`. #6b should branch off `feat/tool-safety-6a`.

---

## 3. What #6 IS (the design, settled with the user)

#6 became an **umbrella** decomposed into four stacked, independently-shippable increments that **degrade gracefully**: `#6d escalates → without it #6c/#6b block → without them #6a's binary mask applies`. #6a is the permanent safety floor.

Full spec (source of truth): [`.kiro/specs/tool-safety-modes/`](../specs/tool-safety-modes/) — `requirements.md`, `design.md`, `tasks.md`.

**Six cross-cutting Core Principles** (invariants across all increments):
1. **The LLM is not a Pardoner** — a risk verdict may only make an action *stricter*, never loosen a deterministic decision.
2. **Deterministic floor first, LLM second** — policy + static classification run before any LLM is consulted.
3. **Blast radius is action-intrinsic; authority grows toward the root** — danger does NOT correlate with delegation depth (delegation is task/functional), but the autonomous ceiling increases toward the orchestrator (more context up top).
4. **Reversibility must be proven, not asserted** — only real rollback artifacts count (tool-intrinsic, or agent-manufactured backup/staging/worktree). Design answer chosen = option (c): proven, not merely claimed.
5. **Fail toward escalation, not action** — unavailable judgment / over-ceiling escalates; absent a channel it blocks; never silently permitted.
6. **Escalation suspends only the branch** — sibling parallel work continues.

**The four increments:**
- **#6a — Deterministic capability mask (DONE this session).** Binary `readonly`/`mutating`; sub-agents read-only by default; **unclassified tools reserved to humans** (stricter than `mutating`).
- **#6b — Tiers + reversibility + policy + ceiling.** 5-tier ordered blast radius `Safe < Reversible < Disruptive < Destructive < Catastrophic`; **reversibility is an orthogonal *proven* boolean** (not a tier point) that lowers the *authority required*, never the radius; a **Protected Policy File** (own thing, non-pardonable, owner-only, can only raise/forbid); **root-favoring authority ceiling** propagated down the spawn chain (parent may only lower). Over-ceiling/forbidden → block (no escalation yet). Fully deterministic, no LLM. Also defines the escalation-record schema *including* nonce/signature fields (reserved for #6d).
- **#6c — `%assess-risk%` LLM evaluator (stricter-only overlay).** A new role shaped like `assets/roles/%explain-shell%.md`; a **dedicated cheap model** (`safety.risk_model` config); **minimal context** (tool + resolved args + static tier + reversibility + this-step intent only — NEVER full plan/history); structured verdict `{ tier, reversible, confidence, rationale, concerns[], enrichment }`; **stricter-only clamp enforced in Rust**; `Safe`/reads skip the evaluator (fast-path); **plan-time pass flags key steps, then mandatory pre-exec re-check of only the flagged steps** (policy floor not re-checked); fail/low-confidence → escalate (block pre-#6d).
- **#6d — Escalation & control protocol + human-in-the-loop.** **File-based** parent↔child rendezvous in `$XDG_RUNTIME_DIR` (`0600`, atomic, per-branch), **HMAC-authenticated from day one** with a per-tree secret passed via env (`AICHAT_TREE_SECRET`) so forged/injected `CONTINUE` verdicts are rejected; child **stays alive polling** for a verdict; parent verbs **HALT** (graceful) / **REVERT** (child rolls back via its artifact) / **CONTINUE** (child resumes) — REVERT and RESUME executed BY THE CHILD; unresponsive child → hard-kill; escalation **propagates upward** accumulating an evidence trace to the orchestrator; then **human** either as a **branch-blocking interactive CLI prompt** (siblings keep running) OR the same record emitted to a **Layer 3** supervisor when headless. **Branch-scoped suspension** (siblings unaffected).

**Threats stated explicitly:** prompt injection into the evaluator (mitigated by minimal context + stricter-only clamp + non-pardonable floor); forged control files (owner-only atomic + HMAC); cost/latency (`Safe` fast-path + plan-time batch + flagged-only re-check + cheap evaluator model).

**Design decisions the user explicitly settled (do not re-litigate):**
- ARGC **dropped** for this work — the control protocol is Rust-internal; `%assess-risk%` is a plain role asset.
- Control-file transport: **file** (not kill-only, not stdin/stdout).
- Config: a **top-level `safety:` section** (a sibling of `agent_loop:`, NOT nested).
- Unclassified tools: **reserved to humans for now** (the most conservative default; may be relaxed later as policy).
- Reversibility: **proven** (option c).
- Human path is in-scope for the engine (interactive CLI), with optional flow to Layer 3.
- Decomposition tracked as **sub-items** #6a–#6d under one **umbrella** spec folder.

---

## 4. #6a — What Was Implemented & Verified (this session's code)

**Committed at `8994922` on `feat/tool-safety-6a`.** 6 files, +397/−16.

- **`src/function.rs`:**
  - `ToolMode { Readonly, Mutating }` (serde `rename_all = "lowercase"`).
  - New skip-serialized field `mode: Option<ToolMode>` on `FunctionDeclaration` (invisible to the LLM, like `agent`/`output`).
  - `SafetyClass { Readonly, Mutating, Unclassified }` + `FunctionDeclaration::safety_class()` (absent `mode` → `Unclassified`) + `SafetyClass::allowed_under_readonly_mask()` (only `Readonly` true). `Unclassified` is distinct from and stricter than `Mutating` so #6b can apply the reserved-to-humans policy specifically to it.
  - +5 tests (readonly/mutating parse, absent→unclassified, skip-serialize, unclassified-vs-mutating distinctness).
- **`src/agent_loop.rs`:**
  - `eval_agent_tool_subprocess` now sets `cmd.env("AICHAT_CAPABILITY_MASK", "readonly")` on every spawned child (alongside `AICHAT_AGENT_DEPTH`). **Monotonic** — descendants stay masked.
  - New helpers: `under_readonly_mask()` (reads the env var), `tool_safety_class(config, name)` (agent funcs → global funcs → else `Unclassified`; MCP tools live in a separate registry so resolve to `Unclassified`), `capability_denied_result(config, name)`.
  - **Gate is the first thing `eval_single_tool` does:** if masked and the tool is not `readonly`, returns `Ok({"error":{"type":"capability_denied","reason":"mutating"|"unclassified","message":...}})` — a structured result, NOT an `Err`, so it reaches the model verbatim (mirrors the circuit-breaker short-circuit). `_plan` and `readonly` always permitted.
  - `plan_tool_declaration()` now sets `mode: Some(ToolMode::Readonly)`.
  - +3 tests (`tool_safety_class_resolves_from_config`, `capability_gate_permits_everything_when_unmasked`, `capability_gate_denies_mutating_and_unclassified_when_masked`). A `static MASK_ENV_LOCK: parking_lot::Mutex<()>` serializes the env-var-touching tests against the parallel runner.
- **`src/mcp.rs`:** MCP tool entry construction sets `mode: None` (unclassified).
- **Docs:** `.kiro/architecture.md` gained a "Tool safety modes & capability mask (backlog #6a)" section + updated the dispatch diagram (gate + `AICHAT_CAPABILITY_MASK` on child spawn); depth note updated. `.kiro/docs/progress.md` #6a row → Implemented (unmerged), Tests 352→360, current branch updated. Spec `tasks.md` #6a boxes all checked.

**Verified:** full `cargo test --bin aichat` = **352 unit pass, 0 fail** (was 344; +8 new #6a tests → workspace total **360** incl. 5 catalog-override + 3 integration). `cargo clippy --bin aichat`: no errors, **no new warnings** — the 4 style lints (`map_or` at agent_loop.rs 434/439/690, `collapsible_if` at 815) and all dead-code warnings are PRE-EXISTING and unrelated (434/439 verified = the existing `is_agent` check). Pre-existing warnings left untouched to keep the diff clean.

---

## 5. Backlog State at Handoff

Source of truth: [`.kiro/docs/backlog.md`](backlog.md) and [`.kiro/docs/progress.md`](progress.md) (progress.md now has #6/#6a/#6b/#6c/#6d rows).

| # | Item | Status | Priority |
|---|------|--------|----------|
| 1 | Rust MCP Bridge | ✓ Done (merged) | High |
| 3 | Client-Side Agent Loop | ✓ Done (merged) | High |
| 4 | Tool Output Routing | ✓ Done (merged) | Medium |
| 5 | Test Suite & Coverage Hardening | ✓ Done (merged) | Medium |
| 6 | Tool Safety Modes & Actuation Governance (umbrella) | Spec written; **#6a implemented (unmerged)** | **High** |
| 6a | ↳ Deterministic capability mask | ✓ Implemented on `feat/tool-safety-6a` (unmerged) | High |
| 6b | ↳ Tiers + reversibility + policy + ceiling | Proposed (next) | High |
| 6c | ↳ `%assess-risk%` LLM evaluator | Proposed | High |
| 6d | ↳ Escalation/control + human-in-the-loop | Proposed | High |
| 7 | Session Resumption & WAL Journaling | Proposed | High |
| 8 | Dynamic Multi-Turn Context Compaction | Proposed | Medium |
| 9 | Ephemeral Git Worktree Isolation | Proposed | Medium |
| 10 | Staged Config & Dry-Run Protocol | Proposed | Medium |
| 11 | Mock-Client Test Seam for Loop Coverage | Proposed | Low |
| 2 | Gemini Interactions API | Deferred | Low |

**Recommended next work:** **#6b** — branch `feat/tool-safety-6b` off `feat/tool-safety-6a`. Implement the 5-tier `BlastRadius` (with `Ord`), orthogonal proven-reversibility (`reversible`/`reversible_via` fields + `required_authority(tier, proven_reversible)` in a new `src/safety.rs`), the Protected Policy File loader (owner-only, raise-only), the top-level `safety:` config section, the `AICHAT_AUTHORITY_CEILING` env propagation + over-ceiling block, and reserve the `EscalationRecord`/`RiskVerdict` schemas (with nonce/signature). Follow the phased checklist in `tasks.md` Phase #6b. Keep the suite green and confirm the degrade-check at the end.

Note #6b compatibility rule from the spec: legacy `mode` maps onto the tier scale (`readonly`→`Safe`, `mutating`→ at least `Disruptive`) so #6a declarations keep working.

---

## 6. Open Items / Known Gaps (none block #6b)

1. **Unpushed local-only state** — `main` 140 ahead of `origin/main`; `feat/tool-safety-6a` a further 2 ahead. No remote backup. Intentional.
2. **`$0.000000` cost-estimator bug** — cost display reports zero for Gemini despite real token usage (seen in Session 3's live demo). Still uninvestigated, still NOT in the backlog. Relevant to #6c evaluator cost and any `max_cost` work. Worth logging.
3. **#6a trace nuance (minor, acceptable):** a `capability_denied` result flows through the parallel dispatcher's `Ok` branch, so it emits `ToolComplete { success: true }` and, because it carries an `error` key, counts toward the per-tool circuit breaker. Behaviorally fine for #6a (a denied tool isn't a crash; repeated denials tripping the breaker is acceptable). Revisit only if it proves noisy.
4. **Three intentionally-untracked files** remain (`agent-loop-operation.mmd`, `AI.pdf`, `manual.pdf`) — do not commit them.

---

## 7. Key Files for Continuation

- **The #6 spec (READ FIRST for #6b):** [`.kiro/specs/tool-safety-modes/`](../specs/tool-safety-modes/) — requirements/design/tasks. Design has the decision-funnel diagram, the `FunctionDeclaration` field plan, `required_authority` combination, the env-propagation table, per-FR seam map, and the threat model.
- **Spec template convention:** [`.kiro/specs/test-suite-hardening/`](../specs/test-suite-hardening/).
- **#6a code:** `src/function.rs` (`ToolMode`/`SafetyClass`), `src/agent_loop.rs` (mask propagation in `eval_agent_tool_subprocess`; gate + helpers just above `eval_single_tool`), `src/mcp.rs`.
- **Config seam for #6b:** `src/config/mod.rs` — `AgentLoopConfig` is at ~line 277 (`#[serde(default, deny_unknown_fields)]`); the new **top-level `safety:` section** goes here as a sibling `SafetyConfig`.
- **Evaluator role template for #6c:** `assets/roles/%explain-shell%.md`.
- Architecture: [`.kiro/architecture.md`](../architecture.md) (now documents the #6a mask), [`.kiro/docs/fork-philosophy-and-architecture.md`](fork-philosophy-and-architecture.md) (Tenet 4, Pillars 2/5 anchor #6).
- Backlog & status: [`.kiro/docs/backlog.md`](backlog.md) (#6 rewritten as umbrella), [`.kiro/docs/progress.md`](progress.md).

### Environment notes
- **Nushell environment.** `&&`, `2>&1`, `2>/dev/null` do NOT work directly — wrap shell pipelines/redirects in `bash -c "…"`; use `;` between nu statements; `print` not `echo`.
- Tests: `cargo test --bin aichat` (binary crate; no `--lib`). Use debug profile (release compile is slow / has timed out here).
- Dev functions: `~/projects/llm-functions` (safe, use `AICHAT_FUNCTIONS_DIR`). Live functions `~/clones/llm-functions` (**do not touch**).
- Standing user preferences: **local-only, do not push or commit without asking**; each increment is a valid stopping point; do not start the next phase's implementation until the current one is green + degrade-checked.
