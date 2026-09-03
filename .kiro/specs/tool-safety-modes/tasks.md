# Tool Safety Modes & Actuation Governance — Tasks

Umbrella branch strategy: one feature branch per increment, each off the previous, so each
is independently reviewable and shippable and later ones stack:

```
main
 └─ feat/tool-safety-6a   (capability mask — the floor)
     └─ feat/tool-safety-6b   (tiers + reversibility + policy file + ceiling)
         └─ feat/tool-safety-6c   (%assess-risk% evaluator overlay)
             └─ feat/tool-safety-6d   (escalation/control + human-in-the-loop)
```

Spec-before-implementation: this spec is committed first. Each increment MUST keep the full
suite green (NFR-1) and MUST remain correct with all later increments absent (NFR-6).

---

## Phase #6a — Deterministic Capability Mask (branch `feat/tool-safety-6a`)

- [x] 6a.1 Add `ToolMode { Readonly, Mutating(default) }` and `mode: Option<ToolMode>` to
      `FunctionDeclaration` (`#[serde(skip_serializing, default)]`). (FR-6a.1) Also added
      `SafetyClass { Readonly, Mutating, Unclassified }` so *absent* mode is distinguishable from
      an explicit `mutating` and carries the stricter reserved-to-humans disposition.
- [x] 6a.2 Unit tests: JSON round-trip; a tool declaring a bare mode → `Mutating`; an **unclassified**
      tool (no `mode`) and MCP-sourced tools → reserved-to-humans disposition (blocked in masked sub-agent). (FR-6a.2)
- [x] 6a.3 In `eval_agent_tool_subprocess`, set `AICHAT_CAPABILITY_MASK=readonly` on the child; leave
      top-level (depth 0) unmasked. Monotonic — descendants stay masked. (FR-6a.3, FR-6a.5)
- [x] 6a.4 Add a capability gate in `eval_single_tool` (before MCP/agent/shell routes): if masked and the
      resolved tool is `mutating`/unclassified, return `{"error":{"type":"capability_denied",…}}` without
      executing — mirror the tripped-tool short-circuit shape. `_plan` always permitted. (FR-6a.4)
- [x] 6a.5 Unit tests: masked context denies mutating + unclassified, permits readonly + `_plan`; unmasked
      permits all; `tool_safety_class` resolves from config. Env-var tests serialized via a mutex.
- [x] 6a.6 `cargo test` (352 unit, 0 fail; +8) + `cargo clippy` (no new warnings) green. Standalone
      behavior confirmed. (FR-6a.6, NFR-1)
- [x] 6a.7 Docs: capability mask, dispatch gate, and `mode` metadata documented in `.kiro/architecture.md`;
      `progress.md` #6a status + test count updated.

## Phase #6b — Tiers, Reversibility, Protected Policy, Authority Gradient (branch `feat/tool-safety-6b`)

- [ ] 6b.1 Add `BlastRadius { Safe<Reversible<Disruptive<Destructive<Catastrophic }` (`Ord`) and
      `reversible: Option<bool>` / `reversible_via: Option<String>` to `FunctionDeclaration`. (FR-6b.1/6b.3)
- [ ] 6b.2 Map legacy `mode` → tier (`readonly`→`Safe`, `mutating`→≥`Disruptive`) for back-compat; unit-test. (FR-6b.1)
- [ ] 6b.3 Create `src/safety.rs`; implement pure `required_authority(tier, proven_reversible)` and unit-test the
      orthogonal combination (proof lowers required authority one step; never changes tier). (FR-6b.2)
- [ ] 6b.4 Protected Policy File: loader (owner-only perm check) + `policy_tier(action) -> Raise(tier)|Forbidden`,
      raise-only semantics; built-in fail-safe defaults when absent. Unit tests incl. a forbidden rule. (FR-6b.4)
- [ ] 6b.5 Add a top-level `SafetyConfig` (its own `safety:` section, sibling of `agent_loop:`) with
      `serde(default)` safe defaults; wire `default_ceiling`. Unit-test defaults + partial override. (FR-6b.5, NFR-7)
- [ ] 6b.6 Propagate `AICHAT_AUTHORITY_CEILING` to children (parent may only lower); enforce
      `required_authority(action) <= ceiling` in dispatch, else `authority_exceeded`; `policy_forbidden`
      for policy hits. Unit tests for over-ceiling block and child-ceiling-lowering. (FR-6b.5/6b.6)
- [ ] 6b.7 Define `EscalationRecord`/`RiskVerdict` schemas including `nonce`/`signature` fields (unused now). (FR-6b.7)
- [ ] 6b.8 `cargo test` + `cargo clippy` green; degrade check: with no policy file and no evaluator, behavior is
      deterministic tiers + block. (NFR-1/6)
- [ ] 6b.9 Docs: risk/reversibility metadata, policy-file format, ceiling model.

## Phase #6c — `%assess-risk%` LLM Evaluator (branch `feat/tool-safety-6c`)

- [ ] 6c.1 Add `assets/roles/%assess-risk%.md` — terse, structured-verdict prompt (shape of `%explain-shell%`). (FR-6c.1)
- [ ] 6c.2 Add `safety.risk_model` config; evaluator invocation builds a minimal `Input` with that model;
      absent model → evaluator skipped (degrade to #6b). (FR-6c.2)
- [ ] 6c.3 Minimal-context builder in `safety.rs`: only {tool, resolved args, static tier, reversibility,
      this-step intent}; explicitly exclude plan/conversation history. Unit-test the payload shape. (FR-6c.3)
- [ ] 6c.4 `RiskVerdict` parse: tolerate malformed/partial output → `confidence: low` (no panic). Unit tests. (FR-6c.4/6c.8)
- [ ] 6c.5 **Stricter-only clamp** (pure fn): `effective_tier = max(static_or_policy, verdict.tier)`; proof may be
      withheld, never granted; policy never loosened. Exhaustive unit tests incl. a permissive verdict = no-op. (FR-6c.5)
- [ ] 6c.6 `Safe` fast-path: assert (via a mock evaluator seam) the evaluator is **not** called for `Safe`/reads. (FR-6c.6)
- [ ] 6c.7 Two-phase: plan-time pass flags key steps; only flagged steps re-evaluated at act-time (policy floor not
      re-checked). Unit-test the flag→recheck wiring with a mock verdict. (FR-6c.7)
- [ ] 6c.8 Fail-toward: evaluator error/timeout/low-confidence does not permit; blocks pre-#6d. Unit test. (FR-6c.8)
- [ ] 6c.9 `cargo test` + `cargo clippy` green; degrade check: no `risk_model` ⇒ exactly #6b behavior. (NFR-1/6)
- [ ] 6c.10 Docs + threat note (prompt injection mitigations: minimal context, clamp, policy floor). (NFR-2)

## Phase #6d — Escalation & Control Protocol + Human-in-the-Loop (branch `feat/tool-safety-6d`)

- [ ] 6d.1 File rendezvous in `safety.rs`: atomic `0600` writes, per-branch unique paths under
      `$XDG_RUNTIME_DIR` (fallback `/tmp`); write escalation request (WHY + enrichment + proposed action). (FR-6d.2)
- [ ] 6d.2 Adversarial integrity: mint `AICHAT_TREE_SECRET` at spawn (env, never persisted); HMAC over the
      record; verify on read; reject tampered/wrong-secret/forged as no-verdict. Unit tests (accept/tamper/wrong-key). (FR-6d.3, NFR-3)
- [ ] 6d.3 Child suspend-and-poll loop: at a pending over-ceiling/hesitant action, write escalation, stay alive
      polling for a verdict up to `verdict_timeout_secs`. (FR-6d.1/6d.2)
- [ ] 6d.4 Verdict verbs handled **in the child**: HALT (graceful stop before action), REVERT (child rolls back via
      its reversibility artifact), CONTINUE (child resumes + performs). Unit-test dispatch of each. (FR-6d.4)
- [ ] 6d.5 Parent side: merge child enrichment into context, decide or re-escalate upward accumulating the evidence
      trace to the orchestrator. (FR-6d.5)
- [ ] 6d.6 Human-in-the-loop: interactive branch-blocking prompt (siblings keep running) with action/tiers/evidence
      + approve/deny/revert; OR emit the same record to a Layer 3 sink when headless/preferred. (FR-6d.6)
- [ ] 6d.7 Graceful vs hard stop: cooperative HALT; signal-kill an unresponsive child on poll timeout. Test the
      timeout→hard-kill path offline. (FR-6d.7)
- [ ] 6d.8 Branch-scoped suspension: verify a suspended lineage does not block `join_all` siblings. Unit/async test. (FR-6d.8)
- [ ] 6d.9 Offline demo in `scripts/run-demos.nu` (à la Demo 12): real parent↔child escalation over subprocesses,
      asserting verdict verbs + no forged-file acceptance + branch-only suspension. (Verification)
- [ ] 6d.10 `cargo test` + `cargo clippy` green; degrade check: disabling #6d ⇒ over-ceiling/hesitant actions block
      exactly as #6b/#6c. (NFR-1/6)
- [ ] 6d.11 Docs: full funnel diagram, escalation record format, human/Layer-3 paths, threat model.

## Cross-cutting / Land

- [ ] X.1 Update `.kiro/docs/backlog.md`: replace the #6 body with the #6a–#6d decomposition (umbrella + sub-items),
      referencing this spec.
- [ ] X.2 Update `.kiro/docs/progress.md` #6 row to reflect the staged plan and per-phase status.
- [ ] X.3 Update architecture docs with the decision-funnel and the new `src/safety.rs` module.
- [ ] X.4 Session summary entry when the first increment (#6a) lands.

## Notes

- Debug `cargo test` for iteration (release compile is slow in this env; see prior specs).
- No change to live `~/clones/llm-functions`; use dev clone `~/projects/llm-functions` and
  `AICHAT_FUNCTIONS_DIR` for any functions-side testing.
- Keep all tests deterministic/hermetic: evaluator via mock verdict, control-files under isolated
  temp paths with cleanup, no network/live providers/tty.
- Local-only repo state (nothing pushed) per the standing user preference — no push without asking.
- Each increment is a valid stopping point; do not begin the next phase's implementation until the
  current one is green and its degrade-check confirmed.
