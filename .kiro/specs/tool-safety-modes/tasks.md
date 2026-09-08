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

- [x] 6b.1 Add `BlastRadius { Safe<Reversible<Disruptive<Destructive<Catastrophic }` (`Ord`) and
      `reversible: Option<bool>` / `reversible_via: Option<String>` to `FunctionDeclaration`. (FR-6b.1/6b.3)
- [x] 6b.2 Map legacy `mode` → tier (`readonly`→`Safe`, `mutating`→≥`Disruptive`) for back-compat via
      `StaticTier` + `static_tier()`; unit-tested. (FR-6b.1)
- [x] 6b.3 Create `src/safety.rs`; pure `required_authority(static_tier, policy, proven_reversible)` +
      `AuthorityCeiling`; unit-tested the orthogonal combination (proof lowers one step; never changes tier). (FR-6b.2)
- [x] 6b.4 Protected Policy File: owner-only YAML loader + `evaluate()` (raise-or-forbid, strictest wins,
      hand-rolled glob, no new deps); built-in empty-default when absent. Tests incl. forbid + owner-perm reject. (FR-6b.4)
- [x] 6b.5 Top-level `SafetyConfig` (`safety:` section, sibling of `agent_loop:`) with `serde(default)`
      safe defaults; `default_ceiling`=Destructive. Tests: defaults + partial + full override. (FR-6b.5, NFR-7)
- [x] 6b.6 Propagate `AICHAT_AUTHORITY_CEILING` to children (parent only lowers); enforce
      `required_authority <= ceiling` in dispatch → `authority_exceeded`; policy hit → `policy_forbidden`.
      Tests: over-ceiling block, child-ceiling lowering, proven-reversibility discount, unclassified→human. (FR-6b.5/6b.6)
- [x] 6b.7 Define reserved `EscalationMsg`/`VerdictMsg` + `VerdictDecision` WS message schemas (unused, round-trip tested). (FR-6b.7)
- [x] 6b.8 `cargo test` (384 unit, 0 fail; +32) + `cargo clippy` (no new warnings) green; degrade check:
      default config (no policy/no risk_model) = deterministic tiers + block; #6a mask intact. (NFR-1/6)
- [x] 6b.9 Docs: tiers/reversibility/policy-format/ceiling documented in `.kiro/architecture.md` (+ dispatch
      diagram gates); `progress.md` #6b row, test count, branch status updated.

## Phase #6c — `%assess-risk%` LLM Evaluator (branch `feat/tool-safety-6c`)

- [x] 6c.1 Add `assets/roles/%assess-risk%.md` — terse, structured-verdict prompt (shape of `%explain-shell%`). (FR-6c.1)
- [x] 6c.2 Add `safety.risk_model` config; evaluator invocation builds a minimal `Input` with that model;
      absent model → evaluator skipped (degrade to #6b). (FR-6c.2)
- [x] 6c.3 Minimal-context builder in `safety.rs`: only {tool, resolved args, static tier, reversibility,
      this-step intent}; explicitly exclude plan/conversation history. Unit-test the payload shape. (FR-6c.3)
- [x] 6c.4 `RiskVerdict` parse: tolerate malformed/partial output → `confidence: low` (no panic). Unit tests. (FR-6c.4/6c.8)
- [x] 6c.5 **Stricter-only clamp** (pure fn): `effective_tier = max(static_or_policy, verdict.tier)`; proof may be
      withheld, never granted; policy never loosened. Exhaustive unit tests incl. a permissive verdict = no-op. (FR-6c.5)
- [x] 6c.6 `Safe` fast-path: assert (via a mock evaluator seam) the evaluator is **not** called for `Safe`/reads. (FR-6c.6)
- [x] 6c.7 **[As-built: superseded literal plan-time flagging]** Two-phase intent realized as a **monotonic,
      raise-only `RiskCache`** (keyed by tool + resolved args) shared across a run: act-time evaluation is the
      floor; a cache hit reuses the recorded authority *floor* (skips a redundant model call) and can only ever
      *raise*, never green-light. A future whole-plan red-light pre-pass writes into the same cache. Rationale:
      the loop is turn-based ReAct (no structured plan to flag), and letting an unflagged step skip its act-time
      check would be an injection hole. Unit-tested (raise-only, key canonicalization, hit-blocks/hit-proceeds
      without a model call). See design.md "As-Built Notes — #6c". (FR-6c.7)
- [x] 6c.8 Fail-toward: evaluator error/timeout/low-confidence does not permit; blocks pre-#6d. Unit test. (FR-6c.8)
- [x] 6c.9 `cargo test` (418 workspace: 410 unit + 5 + 3, 0 fail) + `cargo clippy` (no new warnings) green;
      degrade check: no `risk_model` ⇒ exactly #6b behavior (all #6a/#6b gate tests unchanged). (NFR-1/6)
- [x] 6c.10 Docs + threat note: `.kiro/architecture.md` #6c section + dispatch gate; roadmap.md status; this
      tasks list; design.md as-built note (prompt-injection mitigations: minimal context, stricter-only clamp,
      fail-toward, raise-only cache). (NFR-2)
- [x] 6c.11 Semantic declaration & implementation context enrichment:
      - `ToolDeclarationContext` + `parse_script_header_comments()` in `src/safety.rs` for `@describe`, header commentary, parameter docs, and `@env` extraction.
      - `resolve_tool_implementation()` in `src/agent_loop.rs`: checks MCP, agent tools/bin, root tools/bin, follows symlinks, detects binaries via null-byte check, enforces 4KB text budget.
      - `format_tool_invocation()` generates exact command preview with bound CLI arguments.
      - Updated `assets/roles/%assess-risk%.md` prompt directives (contrast declared intent with code, trace args to sinks, detect hardcoded side effects and confirmation guards).
      - `[%assess-risk% evaluator response]` trace logging; hermetic end-to-end unit test `evaluator_context_end_to_end_resolution`. (FR-6c.3)
- [x] 6c.12 Complete 100% parameter classification in `llm-functions` (branch `feat/tool-safety-classification`, commit `83dea2e`):
      - All 31 root tools annotated with `# @meta mode` and `# @meta risk` (`fs_patch`, `fs_rm`, `fs_write` have `reversible-via backup`).
      - All agent subcommands (`coder`, `demo`, `json-viewer`, `orchestrator`, `researcher`, `sql`, `todo`) classified.
      - Verified via `argc test` and all 16 demos in `scripts/run-demos.nu`.

## Phase #6d — Escalation & Control Protocol + Human-in-the-Loop (branch `feat/tool-safety-6d`)

> **Transport (Path 1′):** mutual-TLS over a raw loopback TCP stream + hand-rolled length-delimited
> JSON framing (via `tokio-rustls`, already in the tree; only `rcgen` added). NOT WebSocket — WS
> framing is deferred behind an `EscalationTransport` trait until a remote deployment needs it. The
> auth model and message protocol are unchanged from the design. See design.md "As-Built Notes — #6d".

- [x] 6d.1 mTLS listener in the parent: bind loopback (`127.0.0.1:<port>`) at spawn via `tokio-rustls`;
      pass `AICHAT_AGENT_PARENT_ADDR` + `AICHAT_AGENT_TOKEN` + `AICHAT_TREE_SECRET` to the child via env.
      Child dials back; length-delimited JSON framing over the TLS stream. (FR-6d.2)
- [x] 6d.2 Mutual auth (mTLS, no CA): parent generates ephemeral per-tree keypair in-memory (`rcgen`); child
      pins parent fingerprint + presents credential derived from the tree secret; channel-bound
      challenge–response; custom rustls verifier fails closed; reject connections failing the handshake.
      Unit tests (valid connects / wrong-fingerprint rejected / replay rejected). (FR-6d.3, NFR-3)
- [x] 6d.3 Typed message protocol in `safety.rs`: Hello / Event / Escalation / Result upstream; Verdict / Cancel
      downstream. Transport-independent (works over the loopback TLS stream now, routable TLS/WS later).
      Unit-test (de)serialization. (FR-6d.4)
- [x] 6d.4 Child connection handler: on a pending over-ceiling/hesitant action, send `Escalation`, then **block on
      `recv()`** for a `Verdict` (no polling), up to `verdict_timeout_secs`. (FR-6d.1/6d.4)
- [x] 6d.5 Verdict verbs handled **in the child**: HALT (graceful stop before action), REVERT (replay durable
      journal entry), CONTINUE (resume + perform). Unit-test dispatch of each. (FR-6d.5)
- [x] 6d.6 Durable rollback journal (append-only, on-disk, separate from the connection): write entry before/at a
      proven-reversible mutation; REVERT replays it. Survives connection drop / child death / re-spawn. Unit tests. (FR-6d.6)
      *Note:* Engine-level pre-mutation journal logging is live; full inverse tool-defined rollback command/artifact restoration is provided under Pillars #9/#10.
- [x] 6d.7 Parent side: merge child enrichment into context, decide or re-escalate upward its own connection to its
      parent, accumulating the evidence trace to the orchestrator. (FR-6d.7)
      *Note:* Parent upward re-escalation forwards the child payload with agent/depth metadata over persistent connection; multi-hop lineage trace accumulation is a follow-up enhancement.
- [x] 6d.8 Human-in-the-loop: interactive branch-blocking prompt (siblings keep running) with action/tiers/evidence
      + approve/deny/revert; OR emit the same record to a Layer 3 sink when headless/preferred. (FR-6d.8)
- [x] 6d.9 Liveness from connection state: child EOFs on parent death, parent errors on child death (replaces
      `/proc` scanning). Graceful Cancel/HALT via connection; signal-kill an unresponsive child on timeout. Test offline. (FR-6d.9/6d.10)
- [x] 6d.10 Branch-scoped suspension: verify a lineage blocked on a verdict does not block `join_all` siblings. Async test. (FR-6d.11)
- [x] 6d.11 Offline demo in `scripts/run-demos.nu` (à la Demo 12): real parent↔child escalation over a loopback mTLS
      connection between spawned processes — assert mutual-auth rejection of a wrong-fingerprint connection, verdict
      verbs, journal-based REVERT survives a killed child, and branch-only suspension. (Verification, FR-6d.12 hooks)
- [x] 6d.12 `cargo test` + `cargo clippy` green; degrade check: disabling #6d ⇒ over-ceiling/hesitant actions block
      exactly as #6b/#6c. (NFR-1/6)
- [x] 6d.13 Docs: full funnel diagram, the mTLS channel (topology, mutual auth, message protocol, control-vs-durability
      planes, WS-deferred-behind-trait rationale), human/Layer-3 paths, threat model, and the remote-generalization hook. (FR-6d.12)
- [x] 6d.14 Pre-flight opportunistic remediation (Option B): upfront journal pre-mutation backup on ceiling trip when `reversible_via == "backup"`, authority step-down (`one_step_down`), and `PreflightReversibilityApplied` event emission. (FR-6d.13)
- [x] 6d.15 Reversibility-aware verdict clamp (`clamp_verdict` preserving reversibility step-down when evaluator agrees with tool tier; pure fn + unit tests). (FR-6d.14)
- [x] 6d.16 Evaluator context enhancement (`rollback_mechanism`) & permanent model support in `%assess-risk%` role front-matter. (FR-6d.15)
- [x] 6d.17 Structured safety execution trace enrichment (`[safety gate passed: ...]`, `[safety preflight reversibility: ...]`, `[%assess-risk% evaluator response: ...]`). (FR-6d.16)
- [x] 6d.18 Live demo expansion in `scripts/run-demos.nu`: Demos 17 (Happy Path), 18 (Option B Remediation), 19 (Authority Ceiling Fail-Closed), 20 (Orchestrator to Sub-Agent Multi-Process Escalation). (Verification)
- [x] 6d.19 Supervisory policy enforcement & risk assessment in escalation handler ("The Should Gate"): load supervisor PolicyFile, evaluate caller tool + args, compute anti-spoofed static tier and reversibility floor, run %assess-risk% with supervisory context (child ID, depth, stated reason, enrichment), clamp strictly, and route Continue/Halt/Human. (FR-6d.17)

## Cross-cutting / Land

- [x] X.1 ~~Update `.kiro/docs/backlog.md`~~ — **superseded.** `backlog.md` + `progress.md` were
      consolidated into a single [`.kiro/docs/roadmap.md`](../../docs/roadmap.md) (Session 5), which
      carries the #6/#6a–#6d decomposition in its Backlog view + canonical Status Table.
- [x] X.2 ~~Update `.kiro/docs/progress.md`~~ — **superseded** by the same consolidation; per-phase
      status now lives once in the roadmap.md Status Table.
- [x] X.3 Architecture docs updated with the decision-funnel and `src/safety.rs` module (#6a–#6c
      landed; #6d section added in 6d.13).
- [x] X.4 Session summaries: Session 4 (#6a), Session 5 (#6b), Session 6 (#6c + #6d Part 1), Session 7 (#6d Part 2), Session 8 (%assess-risk% Code-Aware Evaluator), Session 9 (Option B Pre-flight Remediation & Demos 17-20), Session 10 (Supervisory Policy Enforcement & Risk Evaluation).

## Notes

- Debug `cargo test` for iteration (release compile is slow in this env; see prior specs).
- No change to live `~/clones/llm-functions`; use dev clone `~/projects/llm-functions` and
  `AICHAT_FUNCTIONS_DIR` for any functions-side testing.
- Keep all tests deterministic/hermetic: evaluator via mock verdict, control-files under isolated
  temp paths with cleanup, no network/live providers/tty.
- Local-only repo state (nothing pushed) per the standing user preference — no push without asking.
- Each increment is a valid stopping point; do not begin the next phase's implementation until the
  current one is green and its degrade-check confirmed.
