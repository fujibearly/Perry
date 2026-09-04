# Session Summary & Agent Handoff (Session 4: 2026-09-02)

**Period:** `2026-09-02` (distinct working session; same calendar day as Session 3 but separate work — spec authoring, first implementation increment of backlog #6, and a design brainstorm that redesigned the #6d escalation channel).
**Repository:** `/home/istari/projects/aichat`
**Branch at handoff:** `feat/tool-safety-6a` @ `e6c8af2` (4 commits ahead of `main`).
**Version:** `v0.31.0-fork.9`

---

## 1. Executive Summary

This session **designed backlog #6 (Tool Safety Modes & Actuation Governance) in depth, implemented its first increment (#6a), then — via an extended design brainstorm — redesigned the #6d escalation channel** from a file-rendezvous to a mutually-authenticated WebSocket, and added a new backlog item **#14 (per-machine audit log)**. The original ~150-250-line binary capability mask grew into a four-part **umbrella** (#6a–#6d) with a layered decision funnel. Sequence of work:

1. Wrote the umbrella spec, folded #6a–#6d into backlog/progress (commit `c28dfd5`).
2. Implemented, tested, documented, committed **#6a** (commit `8994922`).
3. Wrote this handoff summary (commit `e338c8b`).
4. **Brainstormed the inter-agent escalation transport** and redesigned #6d: file-rendezvous → **mTLS WebSocket** (child dials parent); added backlog **#14** (audit log); reconciled all three tracking docs (commit `e6c8af2`).

All work is local-only on the `feat/tool-safety-6a` branch (nothing pushed, per standing user preference). **The user paused here before further implementation.**

---

## 2. Git State (verified at handoff)

- **Current branch:** `feat/tool-safety-6a`, HEAD `e6c8af2`, branched off `main` @ `bef34a3`.
- **4 commits ahead of `main`:**
  1. `c28dfd5` — docs: add spec for backlog #6 (umbrella spec + folds #6a–#6d into backlog.md/progress.md)
  2. `8994922` — feat: backlog #6a — deterministic tool safety capability mask (6 files, +397/−16)
  3. `e338c8b` — docs: add Session 4 handoff summary
  4. `e6c8af2` — docs: revise #6d to mTLS-WebSocket channel; add audit-log backlog #14 (6 files, +197/−82)
- **`main` is 140 commits ahead of `origin/main`. NOTHING PUSHED.** Local-only is deliberate; no remote backup.
- **Working tree clean** except the three long-standing intentionally-untracked files: `.kiro/docs/agent-loop-operation.mmd`, `AI.pdf`, `manual.pdf` (`manual.pdf` used by the demo harness). These were deliberately NOT staged.
- **Branch strategy (from the spec):** one branch per increment, each off the previous —
  `main → feat/tool-safety-6a → feat/tool-safety-6b → …6c → …6d`. #6b should branch off `feat/tool-safety-6a`.
- **NOTE — #6d/#14 docs live on the #6a branch.** The #6d-redesign and #14 doc changes were committed onto `feat/tool-safety-6a` (fine since #6a is unmerged and all local). When #6a merges to `main`, these doc revisions ride along — intended.

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
- **#6d — Escalation & control protocol + human-in-the-loop (REDESIGNED this session — WebSocket).** After a design brainstorm, the transport moved from a file-rendezvous to a **mutually-authenticated WebSocket** channel. Shape:
  - **Topology: child dials parent.** The parent binds a **loopback WSS listener** at spawn and passes address + credentials to the child via env (`AICHAT_AGENT_PARENT_ADDR`, `AICHAT_AGENT_TOKEN`, `AICHAT_TREE_SECRET`). The child connects back after starting. One parent listener, many children. Every agent is both a listener (for its children) and a client (to its parent), so escalation chains recurse naturally.
  - **Mutual auth, no CA/PKI.** Both sides prove identity: parent generates an **ephemeral per-tree keypair** (in-memory, never on disk); child pins the parent's fingerprint and presents a credential derived from the per-tree secret; a **channel-bound challenge–response** makes a leaked credential non-replayable on another connection. Loopback-only binding as defense-in-depth. Chosen because a token *will* leak eventually — this is "stronger than a token, without overkill" (no cert files, no CA, no rotation).
  - **Typed message protocol (transport-independent):** child→parent `Hello`/`Event(AgentLoopEvent)`/`Escalation{id,action,reason,enrichment,blast_radius,reversible}`/`Result`/`Error`; parent→child `Verdict{escalation_id, Halt|Revert|Continue, added_context?}`/`Cancel`. Child hits a gated action, sends `Escalation`, then **blocks on `recv()`** (no polling).
  - **Verbs execute in the child:** HALT (graceful stop), REVERT (child rolls back), CONTINUE (child resumes). **REVERT replays a durable on-disk rollback journal entry — NOT in-memory state** — so reversal survives connection drops, child crashes, and re-spawns.
  - **Three planes, kept separate:** control+telemetry = the ephemeral WSS channel; durability = the on-disk rollback journal; audit = #14. The channel is NOT the source of truth for reversibility or history.
  - Escalation **propagates upward** (child re-escalates up its own connection) accumulating an evidence trace to the orchestrator; then **human** — a **branch-blocking interactive CLI prompt** (siblings keep running) OR the same record to a **Layer 3** supervisor when headless.
  - **Liveness is free** from connection state (EOF on either side's death — replaces `/proc` scanning). **Branch-scoped suspension** — a lineage awaiting a verdict never blocks siblings.
  - **Forward-compatible with remote agents:** the same protocol + auth model generalizes to remote sub-agents (WSS on a routable interface, real certs, agent-registry `endpoint:` field). NOT built in #6d, but the protocol must not preclude it (FR-6d.12). This ties into #12 (Remote MCP / WSS transport muscle).

**Threats stated explicitly:** prompt injection into the evaluator (mitigated by minimal context + stricter-only clamp + non-pardonable floor); **forged control messages / unauthorized connection** (mitigated by mTLS + channel-bound challenge–response + loopback-only — connection failing the handshake is rejected before any message); cost/latency (`Safe` fast-path + plan-time batch + flagged-only re-check + cheap evaluator model).

**Design decisions the user explicitly settled (do not re-litigate):**
- ARGC **dropped** for this work — the control protocol is Rust-internal; `%assess-risk%` is a plain role asset.
- **#6d transport: mutually-authenticated WebSocket, child dials parent** (superseded the earlier file-rendezvous decision). WSS everywhere (loopback local; routable for future remote). Both sides prove identity via **mTLS with ephemeral fingerprint-pinned keys, no CA/PKI**, + channel-bound challenge–response. Rationale: a token alone will leak; needs to be stronger, without overkill.
- **Inter-agent channel is control + telemetry ONLY, not a data plane.** It carries task lifecycle, escalation/verdicts, structured cost, live events, cancellation, health. It does NOT carry tool execution internals or bulk artifacts (those stay isolated per Pillars 1/2, or move via #4 output routing / #13 artifact store). The user explicitly decided **not** to move more onto it.
- **Reversibility externalized to a durable on-disk journal** (not in-memory) so REVERT works even if the child died — this is *why* the child no longer needs to "stay alive" for REVERT; the connection is control-only, the journal is durability.
- Config: a **top-level `safety:` section** (a sibling of `agent_loop:`, NOT nested).
- Unclassified tools: **reserved to humans for now** (the most conservative default; may be relaxed later as policy).
- Reversibility: **proven** (option c).
- Human path is in-scope for the engine (interactive CLI, branch-blocking), with optional flow to a Layer 3 supervisor.
- Decomposition tracked as **sub-items** #6a–#6d under one **umbrella** spec folder.
- **New backlog item #14 (per-machine audit log)** — auditability is a *distinct goal* from observability; durable append-only JSONL per agent, consolidated per-machine via correlation IDs, read by external auditors/platforms. Deferred (future enhancement), needs its own spec. Homes the `$0.000000` cost-bug fix.

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

Source of truth (now consolidated): [`.kiro/docs/roadmap.md`](roadmap.md) — Status Table + Backlog views (has #6/#6a/#6b/#6c/#6d). *(Formerly the separate `backlog.md` + `progress.md`.)*

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
| 12 | Remote MCP Transports (HTTP/WSS) | Proposed | Medium |
| 13 | Scoped Shared Artifact Store | Proposed | Low |
| 14 | Per-Machine Consolidated Audit Log (auditability) | Proposed (new this session) | Medium |
| 2 | Gemini Interactions API | Deferred | Low |

**Recommended next work:** **#6b** — branch `feat/tool-safety-6b` off `feat/tool-safety-6a`. Implement the 5-tier `BlastRadius` (with `Ord`), orthogonal proven-reversibility (`reversible`/`reversible_via` fields + `required_authority(tier, proven_reversible)` in a new `src/safety.rs`), the Protected Policy File loader (owner-only, raise-only), the top-level `safety:` config section, the `AICHAT_AUTHORITY_CEILING` env propagation + over-ceiling block, and reserve the **typed `Escalation`/`Verdict` message schemas** (with the auth/nonce fields for #6d's mTLS handshake — NOTE: schema is now for WebSocket messages, not on-disk records). Follow the phased checklist in `tasks.md` Phase #6b. Keep the suite green and confirm the degrade-check at the end.

Note #6b compatibility rule from the spec: legacy `mode` maps onto the tier scale (`readonly`→`Safe`, `mutating`→ at least `Disruptive`) so #6a declarations keep working.

---

## 6. Open Items / Known Gaps (none block #6b)

1. **Unpushed local-only state** — `main` 140 ahead of `origin/main`; `feat/tool-safety-6a` a further 4 ahead. No remote backup. Intentional.
2. **`$0.000000` cost-estimator bug** — cost display reports zero for Gemini despite real token usage (seen in Session 3's live demo). Still uninvestigated. **Now homed under backlog #14** (the audit-log item is the natural place to fix it via structured cost records instead of stderr scraping). Also relevant to #6c evaluator cost and any `max_cost` work.
3. **#6a trace nuance (minor, acceptable):** a `capability_denied` result flows through the parallel dispatcher's `Ok` branch, so it emits `ToolComplete { success: true }` and, because it carries an `error` key, counts toward the per-tool circuit breaker. Behaviorally fine for #6a (a denied tool isn't a crash; repeated denials tripping the breaker is acceptable). Revisit only if it proves noisy.
4. **Three intentionally-untracked files** remain (`agent-loop-operation.mmd`, `AI.pdf`, `manual.pdf`) — do not commit them.

---

## 7. Key Files for Continuation

- **The #6 spec (READ FIRST for #6b):** [`.kiro/specs/tool-safety-modes/`](../specs/tool-safety-modes/) — requirements/design/tasks. Design has the decision-funnel diagram, the `FunctionDeclaration` field plan, `required_authority` combination, the env-propagation table, per-FR seam map, and the threat model.
- **Spec template convention:** [`.kiro/specs/test-suite-hardening/`](../specs/test-suite-hardening/).
- **#6a code:** `src/function.rs` (`ToolMode`/`SafetyClass`), `src/agent_loop.rs` (mask propagation in `eval_agent_tool_subprocess`; gate + helpers just above `eval_single_tool`), `src/mcp.rs`.
- **Config seam for #6b:** `src/config/mod.rs` — `AgentLoopConfig` is at ~line 277 (`#[serde(default, deny_unknown_fields)]`); the new **top-level `safety:` section** goes here as a sibling `SafetyConfig`.
- **Evaluator role template for #6c:** `assets/roles/%explain-shell%.md`.
- **For #6d (later) — WebSocket stack already available:** the crate already depends on `hyper`/`hyper-util` + `tokio` (see `src/serve.rs`, which runs an OpenAI-compatible HTTP/SSE server with graceful shutdown and `serve_connection_with_upgrades`), and `reqwest`/rustls for TLS. So the WSS listener + mTLS pieces reuse existing deps — likely needs a WebSocket helper crate (e.g. `tokio-tungstenite`) but no new runtime. `src/serve.rs` is the reference for the hyper server + SSE + upgrade patterns.
- Architecture: [`.kiro/architecture.md`](../architecture.md) (now documents the #6a mask), [`.kiro/docs/fork-philosophy-and-architecture.md`](fork-philosophy-and-architecture.md) (Tenet 4, Pillars 2/5 anchor #6).
- Backlog & status: [`.kiro/docs/roadmap.md`](roadmap.md) (consolidated — Roadmap + Traction/Status Table + Backlog views; #6 detailed there).

### Environment notes
- **Nushell environment.** `&&`, `2>&1`, `2>/dev/null` do NOT work directly — wrap shell pipelines/redirects in `bash -c "…"`; use `;` between nu statements; `print` not `echo`.
- Tests: `cargo test --bin aichat` (binary crate; no `--lib`). Use debug profile (release compile is slow / has timed out here).
- Dev functions: `~/projects/llm-functions` (safe, use `AICHAT_FUNCTIONS_DIR`). Live functions `~/clones/llm-functions` (**do not touch**).
- Standing user preferences: **local-only, do not push or commit without asking**; each increment is a valid stopping point; do not start the next phase's implementation until the current one is green + degrade-checked.
