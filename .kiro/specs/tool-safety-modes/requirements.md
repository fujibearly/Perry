# Tool Safety Modes & Actuation Governance — Requirements

## Summary

Backlog item **#6** (umbrella). Give the fork a graduated, safety-governed actuation
model so that autonomous agents — especially parallel sub-agents operating across real
infrastructure — cannot cause catastrophe. The end state is a layered decision funnel:
a deterministic, non-pardonable policy floor; a blast-radius classification of every
action with a *proven*-reversibility axis; an LLM risk evaluator that can only make
verdicts **stricter, never more permissive**; and an escalation/control protocol that
lets a hesitating agent hand a decision up its invoking chain (ultimately to a human)
instead of guessing.

The work is deliberately **decomposed into four stacked increments (#6a–#6d)**, each
independently shippable, each a strict enhancement of the previous, and each degrading
gracefully to the one below it if its richer machinery is absent or fails:

> **#6d escalates → without it #6c/#6b _block_ → without them #6a's binary mask applies.**

This means the simplest, originally-envisioned behavior (a read-only capability mask on
sub-agents) is the permanent safety floor, and every later layer is an enhancement that
can fall back to it.

## Context

- The fork's thesis (see [`fork-philosophy-and-architecture.md`](../../docs/fork-philosophy-and-architecture.md))
  is **system-level** operation: an orchestrator delegates to process-isolated sub-agent
  subprocesses (Pillar 2) that touch filesystems, daemons, databases, clusters, and cloud
  APIs. Tenet 4 is **"triage in parallel, actuate in sequence."**
- The engine already enforces *loop* safety (turn budget, cost circuit breaker, per-tool
  failure circuit breaker in `src/agent_loop.rs`) but has **no notion of an action's
  danger** and **no gate on which agent may perform it**. #6 adds that missing layer.
- Prior art / seams this reuses:
  - The `#[serde(skip_serializing, default)]` metadata pattern that added `output:
    Option<OutputRouting>` to `FunctionDeclaration` (backlog #4) — the same channel carries
    safety metadata, invisible to the LLM.
  - The child-env propagation pattern (`AICHAT_AGENT_DEPTH`) in `eval_agent_tool_subprocess`
    — the capability mask / authority ceiling ride the same channel to child PIDs.
  - The tripped-tool short-circuit partition in `run()` — the model for returning a
    structured "blocked" result instead of executing.
  - The `%explain-shell%` role (`assets/roles/%explain-shell%.md`) — the shape template for
    the `%assess-risk%` evaluator role (narrow question in → terse structured judgment out).
  - Reversibility-providing machinery from adjacent backlog items: **#10** (staging +
    `.bak`/validate/atomic-apply) and **#9** (ephemeral git worktrees).

## Core Principles (invariants that hold across all increments)

1. **The LLM is not a Pardoner.** A risk verdict from the evaluator may only move an
   action *toward stricter* (raise risk, demand escalation, block). It may **never**
   loosen a deterministic decision. Anything the Protected Policy File forbids is forbidden,
   full stop — the evaluator is never even consulted on it.
2. **Deterministic floor first, LLM second.** The deterministic gate (policy file + static
   blast-radius classification) runs *before* any LLM is consulted, both to preserve
   principle 1 structurally and to keep the fast-path cheap.
3. **Blast radius is action-intrinsic; authority grows toward the root.** How dangerous an
   action is does **not** correlate with delegation depth (delegation is task/functional).
   But the *ceiling* an agent may act on autonomously **increases toward the orchestrator**,
   on the grounds that higher agents hold more context. A high-radius action deep in the
   tree escalates upward until it reaches an agent whose ceiling covers it — or a human.
4. **Reversibility must be proven, not asserted.** "Reversible in principle" only counts
   when a real rollback mechanism exists: inherent to the tool, or *manufactured* by the
   agent first taking a reversibility step (backup, staged copy, git commit/worktree).
   An LLM or agent merely claiming reversibility is insufficient.
5. **Fail toward escalation, not toward action.** When judgment is unavailable
   (evaluator unreachable/malformed/low-confidence) or the action exceeds the agent's
   ceiling, the action escalates. If no escalation channel is present (earlier increments),
   the action **blocks**. It is never silently permitted.
6. **Escalation suspends only the branch.** A lineage waiting on a verdict does not stop
   sibling parallel work.

## Increment Overview

| Increment | Adds | Deterministic? | On "can't decide" |
|-----------|------|----------------|-------------------|
| **#6a** | Binary `readonly`/`mutating` capability mask; sub-agents read-only by default; unclassified tools reserved to humans | Yes | Block (mutating/unclassified in sub-agent) |
| **#6b** | 5-tier blast radius + orthogonal proven-reversibility; Protected Policy File; root-favoring authority ceiling | Yes | Block (over ceiling) |
| **#6c** | `%assess-risk%` LLM evaluator (stricter-only); plan-time pass flags key steps; mandatory pre-exec re-check of flagged steps | No (advisory overlay) | Block (no channel yet) |
| **#6d** | File-based parent↔child escalation/control (HALT/REVERT/CONTINUE); branch-only suspension; upward propagation; human-in-the-loop (interactive CLI or Layer 3) | N/A (protocol) | Escalate → human |

---

## Functional Requirements

### Phase #6a — Deterministic Capability Mask (the floor / fallback)

- **FR-6a.1 — Mode metadata.** A tool MAY declare a safety mode in its declaration
  (`functions.json` entry): `mode: "readonly" | "mutating"`. The field is parsed into
  `FunctionDeclaration` as `#[serde(skip_serializing, default)]` so the LLM never sees it.
- **FR-6a.2 — Default mode.** A tool with **no** declared mode is treated as **unclassified**,
  the most conservative disposition: for now, an unclassified tool is **reserved to humans** —
  no autonomous agent (including the top-level orchestrator) may actuate it without human
  approval. In #6a (no escalation channel yet) that means an unclassified `mutating`-equivalent
  tool is **blocked** in any masked sub-agent and requires the unmasked top level; the full
  "reserved to humans" disposition is realized once #6d's human path exists. MCP-sourced tools,
  which carry no such metadata, are likewise unclassified. (This is deliberately stricter than a
  plain `mutating` default and may be relaxed later as a policy choice — "for now" is intentional.)
- **FR-6a.3 — Capability mask propagation.** When the orchestrator (or any agent) spawns a
  sub-agent subprocess, the child inherits a capability mask via env
  (`AICHAT_CAPABILITY_MASK=readonly`), alongside the existing `AICHAT_AGENT_DEPTH`.
- **FR-6a.4 — Masked enforcement.** A subprocess running under a `readonly` mask MUST refuse
  to execute any `mutating` tool. The refusal is a structured tool result
  (`{"error": {"type": "capability_denied", ...}}`), mirroring the circuit-breaker short-circuit
  — it does **not** crash the agent, and it is visible to the model so it can choose another
  approach or (later) escalate.
- **FR-6a.5 — Top-level actuation.** The top-level process (no inherited mask, i.e. depth 0)
  runs unmasked and MAY execute `mutating` tools.
- **FR-6a.6 — Fallback semantics.** #6a MUST be fully functional with none of #6b–#6d present,
  and MUST remain the behavior the later layers degrade to.

### Phase #6b — Blast-Radius Tiers, Proven Reversibility, Protected Policy, Authority Gradient

- **FR-6b.1 — Blast-radius tiers.** Replace the binary mode with a 5-level, ordered
  blast-radius classification: `Safe < Reversible < Disruptive < Destructive < Catastrophic`.
  `Safe` = radius 0 (reads, idempotent queries). The tier is the **impact** axis.
  > Compatibility: the #6a `readonly`/`mutating` mode maps onto this scale (`readonly`→`Safe`,
  > `mutating`→ at least `Disruptive`) so #6a declarations keep working.
- **FR-6b.2 — Reversibility as an orthogonal, proven axis.** Reversibility is a **separate
  boolean** from the radius tier — not a point on the scale. An action's reversibility is
  "proven" only when (a) the tool declares intrinsic reversibility metadata, or (b) a real
  rollback artifact exists for this invocation (a taken backup, staged copy, git commit/worktree).
  Proven reversibility **lowers the authority ceiling required** to perform an otherwise
  high-radius action; it never changes the radius tier itself.
- **FR-6b.3 — Tool risk metadata.** Tools MAY declare `risk: <tier>` and reversibility
  (`reversible: true|false` and/or `reversible_via: "<mechanism>"`) via the same
  skip-serialized metadata channel. A tool with **no** declared `risk` is **unclassified** and
  carries the "reserved to humans" disposition (FR-6a.2): its effective required authority is
  above any autonomous ceiling, so it escalates to a human (or blocks pre-#6d). This is stricter
  than defaulting to a concrete tier and is intentional for now.
- **FR-6b.4 — Protected Policy File.** A protected file defines deterministic, **non-pardonable**
  rules (e.g. "paths under `/etc` are Catastrophic", "prod DB connections are forbidden").
  Rules in this file are the hard floor: they can only *raise* an action's effective tier or
  forbid it outright, and nothing downstream (including the #6c evaluator) may loosen them.
  The file MUST be readable only by the owner and its integrity is the engine's responsibility.
- **FR-6b.5 — Authority ceiling with root-favoring gradient.** Each agent has a maximum tier it
  may perform **autonomously**. The ceiling **increases toward the root** (orchestrator highest).
  Ceilings are passed down the spawn chain (env, like the mask) and an agent MAY only lower,
  never raise, the ceiling it grants a child. An action whose effective required authority
  (radius, reduced by proven reversibility) exceeds the agent's ceiling is **not executed**.
- **FR-6b.6 — Deterministic block on over-ceiling (pre-#6d).** Until the escalation protocol
  (#6d) exists, an over-ceiling or policy-forbidden action returns a structured
  `{"error": {"type": "authority_exceeded" | "policy_forbidden", ...}}` result. No LLM is involved.
- **FR-6b.7 — Escalation record format defined (reserved).** #6b defines the on-disk escalation
  record schema — including the **security fields** (per-tree secret / nonce / signature)
  required for the adversarial protection in #6d — even though #6b does not yet act on them.
  This keeps the format stable across increments.

### Phase #6c — `%assess-risk%` LLM Evaluator (stricter-only overlay)

- **FR-6c.1 — Evaluator role.** Add a new role `%assess-risk%` (an `llm-functions`/assets role,
  shaped like `%explain-shell%`) whose sole job is to return a **terse, structured risk verdict**
  for a single proposed action.
- **FR-6c.2 — Dedicated model.** The evaluator uses a **separate, configurable model**
  (`safety.risk_model`), intended to be small/fast/cheap, distinct from the agent's
  orchestration/planning model. Absent configuration, evaluation is skipped (degrade to #6b).
- **FR-6c.3 — Minimal context.** The evaluator receives **only**: the tool name, the resolved
  command/arguments, the static tier + reversibility facts, and the agent's stated intent for
  *this step*. It MUST NOT receive the full plan history or conversation. (Threat: prompt
  injection via arguments/fetched data — see NFRs.)
- **FR-6c.4 — Structured verdict.** The evaluator returns structured JSON:
  `{ tier, reversible, confidence: low|med|high, rationale, concerns[], enrichment }` where
  `enrichment` is a small context payload to help an upstream agent's retry (per #6d).
  The engine acts only on the **structured** fields, never free-text.
- **FR-6c.5 — Stricter-only.** The engine MUST clamp the evaluator's influence so it can only
  *raise* the effective tier or *withhold* proven-reversibility credit — never lower a tier,
  never grant reversibility, never override the Protected Policy File. (Principle 1, enforced
  in code, not by prompt.)
- **FR-6c.6 — Fast-path skip.** `Safe`/read-only actions MUST NOT call the evaluator (cost/latency).
- **FR-6c.7 — Two-phase evaluation.** At **plan time**, the evaluator runs once over the plan and
  **flags the key steps** that require a fresh pre-execution re-check. At **act time**, only the
  flagged steps are re-evaluated (the deterministic policy floor is invariant and is not re-checked).
- **FR-6c.8 — Fail toward escalation/block.** Evaluator unreachable, timeout, malformed output, or
  `confidence: low` MUST NOT permit the action. With #6d present it escalates; without #6d it blocks.

### Phase #6d — Escalation & Control Protocol + Human-in-the-Loop

- **FR-6d.1 — Escalation trigger.** When an action exceeds an agent's ceiling, or the evaluator
  fails/hesitates, the agent **escalates to its invoking agent** rather than deciding.
- **FR-6d.2 — File-based rendezvous (protected surface).** Escalation and verdict are exchanged
  via files in `$XDG_RUNTIME_DIR` (fallback `/tmp`), `0600`, atomically written, on per-branch
  unique paths. The child writes an escalation request (WHY + enrichment context + the proposed
  action); it then **stays alive polling** for a verdict. (No stdin/stdout coupling; no ARGC.)
- **FR-6d.3 — Adversarial integrity.** The parent mints a per-tree secret at spawn and passes it to
  the child out-of-band (env). Verdict files MUST carry an unforgeable authenticator (HMAC/nonce
  over the record) so that a tool, injected content, or a compromised peer **cannot forge a
  `CONTINUE` verdict or downgrade an escalation**. The engine MUST reject any verdict failing
  verification and treat it as no-verdict (keep waiting / escalate further).
- **FR-6d.4 — Parent verdict verbs.** The invoking agent, having merged the child's enrichment into
  its own context (and possibly re-run its own evaluation/attempt), issues one of:
  **HALT** (child stops before the pending action, gracefully), **REVERT** (child performs the
  rollback using its held reversibility artifact), or **CONTINUE** (child resumes and performs the
  action). REVERT and RESUME are executed **by the child**.
- **FR-6d.5 — Upward propagation.** If the invoking agent's own ceiling/context is insufficient, it
  escalates further up the chain, accumulating the evidence trace, until it reaches the orchestrator.
- **FR-6d.6 — Human-in-the-loop.** If the orchestrator cannot decide, escalation reaches a **human**:
  - **Interactive CLI path (default when a human operates the CLI):** a **blocking prompt on that
    branch only** (siblings keep running) presenting the action, tiers, and the accumulated evidence
    trace, with approve / deny / revert.
  - **Layer 3 path (headless/preferred):** emit the *same* structured escalation record to a Layer 3
    supervisor instead of prompting. One escalation format, two sinks.
- **FR-6d.7 — Graceful vs. hard stop.** HALT is cooperative (the child is waiting anyway). A child
  that is **unresponsive** to a HALT within a timeout MAY be hard-killed (signal) as the escape hatch.
- **FR-6d.8 — Branch-scoped suspension.** A suspended, escalating lineage MUST NOT block sibling
  parallel work elsewhere in the tree.

## Non-Functional Requirements

- **NFR-1 — No regression.** All existing tests (currently 352) MUST continue to pass at every
  increment. Each increment builds clean (`cargo build`) and lints (`cargo clippy`).
- **NFR-2 — Prompt-injection resistance (threat).** The evaluator's minimal context + stricter-only
  clamp + non-pardonable policy floor MUST be the stated, tested mitigations against arguments or
  fetched content attempting to talk the evaluator into a low verdict. A low verdict can never
  unlock a policy-forbidden or over-ceiling action.
- **NFR-3 — Control-file integrity (threat).** Escalation/verdict files MUST be owner-only, atomic,
  per-branch-unique, and (from #6d) authenticated so forged control messages are rejected.
- **NFR-4 — Cost/latency bound (threat).** `Safe`/read actions never invoke the evaluator; evaluator
  calls are bounded by the plan-time pass + flagged-only re-check. The evaluator model is separately
  configurable so it can be a cheap model.
- **NFR-5 — Determinism of tests.** New tests MUST be deterministic and hermetic: no live providers,
  no network, no tty, isolated temp paths with cleanup (matching the `test-suite-hardening` conventions).
  Evaluator behavior is tested via the deterministic clamp logic and a mock verdict, never a live LLM.
- **NFR-6 — Graceful degradation is a requirement, not an accident.** Each increment MUST be shippable
  and correct with all later increments absent, degrading exactly as the Increment Overview table states.
- **NFR-7 — Config safety.** New config lives under an `agent_loop`/`safety` section with
  `serde(default)` so existing configs keep working; defaults MUST be the safe choice.

## Out of Scope

- Building the Layer 3 supervisor itself (the engine only emits the escalation record to it).
- The reversibility-*providing* machinery of #9 (worktrees) and #10 (staging/backup) — #6 *consumes*
  proven reversibility but those mechanisms are their own backlog items. #6 defines how a proven
  rollback artifact is recognized, not how every tool creates one.
- Any change to the live `~/clones/llm-functions` directory (dev clone `~/projects/llm-functions` only).
- ARGC-based helpers for the control protocol (explicitly dropped; the protocol is Rust-internal).
- Cross-host / fleet-wide policy (the Protected Policy File is local; fleet policy is a roadmap L4 concern).
