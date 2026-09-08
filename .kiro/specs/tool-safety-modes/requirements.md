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
| **#6d** | mTLS WebSocket inter-agent channel (child dials parent); typed message protocol (Escalation/Verdict/Cancel); durable rollback journal; branch-only suspension; upward propagation; human-in-the-loop (interactive CLI or Layer 3) | N/A (protocol) | Escalate → human |

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
- **FR-6b.6a — Delegation exception (as-built, decision B).** A tool call that targets a
  sub-agent is NOT subject to the authority gate (nor the #6a mask): delegation is orchestration,
  not actuation, and the sub-agent's own actions are gated inside its process. See the As-Built
  Notes in `design.md`.
- **FR-6b.6b — Catastrophic hard floor (as-built).** Proven reversibility MUST NOT discount a
  `Catastrophic`-tier action; catastrophic always requires a human regardless of reversibility.
- **FR-6b.7 — Escalation message schema defined (reserved).** #6b defines the typed escalation
  and verdict message schemas — including the **security-relevant fields** (agent-id, tree-id,
  challenge/nonce) required for the mutual-auth handshake and channel-bound protocol in #6d —
  even though #6b does not yet act on them. This keeps the message format stable across increments
  and ensures #6d is a transport addition, not a schema redesign.

### Phase #6c — `%assess-risk%` LLM Evaluator (stricter-only overlay)

- **FR-6c.1 — Evaluator role.** Add a new role `%assess-risk%` (an `llm-functions`/assets role,
  shaped like `%explain-shell%`) whose sole job is to return a **terse, structured risk verdict**
  for a single proposed action.
- **FR-6c.2 — Dedicated model.** The evaluator uses a **separate, configurable model**
  (`safety.risk_model`), intended to be small/fast/cheap, distinct from the agent's
  orchestration/planning model. Absent configuration, evaluation is skipped (degrade to #6b).
- **FR-6c.3 — Enriched Semantic & Implementation Context (Bounding Black-Box Blind Spots while Mitigating Injection).**
  To eliminate blind-spot evaluation where the evaluator judges an opaque tool name without knowing what it executes,
  the evaluator context is enriched with three bounded structural payloads:
  1. **`declaration`**: Primary functional description (from `# @describe` or OpenAPI schema), multi-line header
     documentation notes/caveats, typed parameter schemas with required status, and `# @env` variables.
  2. **`implementation`**: The tool's underlying script source code resolved dynamically (checking MCP tools,
     agent `tools/` and `bin/`, and root functions `tools/` and `bin/`, traversing runner symlinks to underlying
     scripts like `bin/execute_command -> run-tool.sh -> tools/execute_command.sh`). Detects language (`bash`,
     `python`, `javascript`, etc.) or categorizes as `Binary`, `Mcp`, `Builtin`, or `Unknown`. Imposes a strict
     4KB text budget (sliced at valid UTF-8 character boundaries) and binary executable detection via null-byte
     scanning in the first 512 bytes.
  3. **`invocation`**: A formatted preview string of the exact tool command-line invocation with bound flags
     (e.g. `execute_command --command "git status"`).
  - **Injection Defense Boundary:** The evaluator MUST NOT receive the full plan history, scratchpad, or
    conversation history. Arguments are treated strictly as untrusted data, and the engine-level monotone
    clamp ensures malicious tool code or arguments can only trigger a raise, never loosen security.
- **FR-6c.4 — Structured verdict.** The evaluator returns structured JSON:
  `{ tier, reversible, confidence: low|med|high, rationale, concerns[], enrichment }` where
  `enrichment` is a small context payload to help an upstream agent's retry (per #6d).
  The engine acts only on the **structured** fields, never free-text.
- **FR-6c.5 — Stricter-only.** The engine MUST clamp the evaluator's influence so it can only
  *raise* the effective tier or *withhold* proven-reversibility credit — never lower a tier,
  never grant reversibility, never override the Protected Policy File. (Principle 1, enforced
  in code, not by prompt.)
- **FR-6c.6 — Fast-path skip.** `Safe`/read-only actions MUST NOT call the evaluator (cost/latency).
- **FR-6c.7 — Act-time evaluation & monotonic raise-only cache (as-built).** The literal plan-time
  flagging pass was superseded in implementation: in a ReAct loop, unflagged step skips create an injection
  hazard. Instead, every non-`Safe`, in-ceiling action is evaluated at act-time, backed by a monotonic
  **raise-only `RiskCache`** that reuses assessed authority floors across turns without redundant model calls.
- **FR-6c.8 — Fail toward escalation/block.** Evaluator unreachable, timeout, malformed output, or
  `confidence: low` MUST NOT permit the action. With #6d present it escalates; without #6d it blocks.

### Phase #6d — Escalation & Control Protocol + Human-in-the-Loop

> **As-built transport decision (Path 1′, supersedes the WSS specifics below).** The #6d channel
> is implemented as **mutual-TLS over a raw loopback TCP stream with hand-rolled length-delimited
> JSON framing** — *not* WebSocket. The **security model is unchanged** (mTLS, ephemeral in-memory
> per-tree keypair, fingerprint pinning, channel-bound challenge–response, loopback-only) and the
> **message protocol is unchanged** (the typed `Escalation`/`Verdict`/`Cancel`/`Hello`/`Event`/
> `Result` set, FR-6d.4). Only the *wire framing* differs: a 4-byte length prefix + `serde_json`
> over `tokio_rustls::TlsStream`, instead of the WebSocket framing layer.
>
> **Rationale.** For a parent talking to its own child on loopback, WebSocket's value (browser/HTTP-
> proxy traversal) does not apply; its framing (masking/opcodes/ping-pong/close handshake) is
> overhead we don't need, and `tokio-tungstenite` is a dependency the fork chose to avoid
> (dependency-brittleness concern). TLS — the security-critical, remote-valuable part — is built now
> and reuses the `tokio-rustls` already in the tree (only `rcgen` added, for ephemeral cert
> generation). **The remote goal (FR-6d.12) is preserved** structurally: the transport sits behind an
> `EscalationTransport` trait and the message protocol is transport-independent, so a WebSocket-over-
> routable-TLS transport can be added as a second impl behind the same trait *when a remote
> deployment (proxy/browser in path) actually needs it* — without a redesign. Wherever the FRs below
> say "WSS"/"WebSocket", read "mutual-TLS loopback stream (WS deferred behind the transport trait)".

- **FR-6d.1 — Escalation trigger.** When an action exceeds an agent's ceiling, or the evaluator
  fails/hesitates, the agent **escalates to its invoking agent** rather than deciding.
- **FR-6d.2 — mTLS inter-agent channel (child dials parent).** The parent binds a
  **loopback TLS listener** (`tokio-rustls`) at spawn time and passes the address + per-child
  credentials to the child via its private spawn environment (`AICHAT_AGENT_PARENT_ADDR`,
  `AICHAT_AGENT_TOKEN`, `AICHAT_TREE_SECRET`). The child **connects back** to the parent after
  starting. One parent listener accepts connections from all its children (one connection per child,
  identified by credentials). The connection is **bidirectional**: the child streams events and
  escalation requests *upstream*; the parent pushes verdicts and cancellation *downstream*.
  (Replaces the earlier file-rendezvous design — a persistent connection eliminates polling latency,
  gives free liveness detection via connection state, and — being TLS — generalizes to remote agents.
  WebSocket framing is deferred behind the transport trait per the as-built note above.)
- **FR-6d.3 — Mutual authentication (mTLS, no CA/PKI).** Both sides MUST prove identity:
  - The parent generates an **ephemeral keypair per agent tree** at startup (in memory, never
    written to disk). The child receives the parent's **public-key fingerprint** via env to pin
    the server it dials.
  - The child authenticates via a **client credential** derived from the per-tree secret, verified
    by the parent. The handshake includes a **challenge–response that is channel-bound** to the
    specific TLS session, so a leaked credential cannot be replayed on another connection.
  - A connection failing mutual auth MUST be rejected immediately. A tool, injected content, or
    compromised local peer **cannot forge a `CONTINUE` verdict or inject an escalation** because
    they cannot complete the handshake.
  - The parent binds **loopback only** (`127.0.0.1`, never `0.0.0.0`) as defense-in-depth.
- **FR-6d.4 — Message protocol (typed, transport-independent).**
  - **Child → parent (upstream):**
    `Hello { agent_id, depth, capabilities }` (handshake, first frame after mTLS);
    `Event(AgentLoopEvent)` (live trace — enables tree-wide observability at orchestrator);
    `Escalation { id, action, reason, enrichment, blast_radius, reversible }` (child pauses,
    awaits verdict on the open connection — **no polling**);
    `Result { output, cost }` / `Error` (terminal).
  - **Parent → child (downstream):**
    `Verdict { escalation_id, decision: Halt | Revert | Continue, added_context? }`;
    `Cancel` (cooperative graceful stop — generalizes HALT beyond escalation).
  - The protocol is defined as **transport-independent typed messages** so the same message set
    works over loopback WSS (local) and routable WSS (future remote agents).
- **FR-6d.5 — Parent verdict verbs.** The invoking agent, having merged the child's enrichment into
  its own context (and possibly re-run its own evaluation/attempt), pushes one of:
  **HALT** (child stops before the pending action, gracefully), **REVERT** (child performs the
  rollback by replaying its **durable on-disk journal entry** — reversibility does NOT depend on
  the child's in-memory state or the connection surviving), or **CONTINUE** (child resumes and
  performs the action). REVERT and CONTINUE are executed **by the child**.
- **FR-6d.6 — Durable rollback journal (separate from the control channel).** Mutations that are
  proven-reversible MUST record a rollback entry to a durable on-disk journal **before or at
  execution** (`{ action, artifact_path, undo_command, agent_id, timestamp, signed }`). The
  WebSocket is the **control plane** (fast, ephemeral, live coordination); the journal is the
  **durability plane** (survives crashes, connection drops, child death). REVERT replays a journal
  entry, not in-memory state — so reversal works even if the child crashed and was re-spawned, or
  the connection was lost. (Complements backlog #7 WAL.)
- **FR-6d.7 — Upward propagation.** If the invoking agent's own ceiling/context is insufficient, it
  escalates further up **its own connection to its parent**, accumulating the evidence trace, until
  it reaches the orchestrator. Every agent is both a listener (for its children) and a client (to
  its parent) — escalation chains recurse naturally.
- **FR-6d.8 — Human-in-the-loop.** If the orchestrator cannot decide, escalation reaches a **human**:
  - **Interactive CLI path (default when a human operates the CLI):** a **blocking prompt on that
    branch only** (siblings keep running) presenting the action, tiers, and the accumulated evidence
    trace, with approve / deny / revert.
  - **Layer 3 path (headless/preferred):** emit the *same* structured escalation record to a Layer 3
    supervisor instead of prompting. One escalation format, two sinks.
- **FR-6d.9 — Liveness (free from connection state).** Parent death → child's socket read EOFs
  immediately (no `/proc` scanning, no stale-file cleanup). Child death → parent's connection
  errors immediately. This replaces the `/proc/<pid>`-based liveness checks entirely.
- **FR-6d.10 — Graceful vs. hard stop.** HALT and Cancel are cooperative (pushed over the
  connection). A child that is **unresponsive** to a HALT/Cancel within `verdict_timeout_secs`
  MAY be hard-killed (signal) as the escape hatch.
- **FR-6d.11 — Branch-scoped suspension.** A suspended, escalating lineage (one child's connection
  blocked awaiting a verdict) MUST NOT block sibling parallel work elsewhere in the tree.
- **FR-6d.12 — Future remote generalization.** The protocol MUST be designed so that the same
  message types and auth model generalize to **remote sub-agents** by binding WSS to a routable
  interface with real certificates (org-CA or pinned), and adding an `endpoint:` field to the
  agent registry. This is NOT built in #6d but the protocol MUST NOT preclude it.
- **FR-6d.13 — Pre-flight Opportunistic Remediation (Option B).** When an autonomous tool call trips
  an agent's authority ceiling solely because it is not yet proven reversible (i.e. `static_tier > ceiling`,
  but `one_step_down(static_tier) <= ceiling`), and the tool declares reversible capability (`reversible == true`
  or `reversible_via == "backup"`), the engine MUST NOT immediately fail closed, block, or escalate.
  Instead, the engine opportunistically creates the atomic pre-mutation file backup (copying existing file
  content to the journal artifact or preparing an `rm -f '<path>'` undo command for newly created files)
  in the durable rollback journal before evaluating the authority gate.
  This marks `reversible = true`, emits `AgentLoopEvent::PreflightReversibilityApplied`, steps down
  the required authority to `one_step_down(static_tier)`, and permits autonomous actuation if within ceiling.
- **FR-6d.14 — Reversibility-Aware Verdict Clamping.** `clamp_verdict` MUST accept the effective
  `reversible: bool` status. When `reversible == true`, the evaluator's raw risk assessment (e.g. `Disruptive`)
  is stepped down by one tier (`one_step_down(verdict.tier) = Reversible`) unless it is `Catastrophic`
  (which remains `Human`). Clamping `stricter_of(base, verdict_required)` preserves the reversibility discount
  when the evaluator agrees with the tool's declared blast radius, preventing the monotone clamp from
  accidentally erasing the reversibility step-down.
- **FR-6d.15 — Evaluator Awareness of Rollback Mechanism.** In `build_evaluator_context`, when
  reversibility has been established or declared, include `"rollback_mechanism": "atomic pre-mutation backup in durable rollback journal"`
  in the evaluator payload so the LLM evaluator does not penalize actions under the false assumption that
  mutations lack rollback artifacts.
- **FR-6d.16 — Structured Safety Trace Enrichment.** Live loop tracing (`AICHAT_AGENT_LOOP_SHOW_TRACE=true`)
  MUST emit structured safety events at each decision boundary:
  `[safety gate passed: <tool> (tier: <tier>, required: <req>, ceiling: <ceiling>)]`,
  `[safety preflight reversibility: atomic backup recorded in journal, required authority stepped down <from> -> <to>]`,
  `[%assess-risk% evaluator response: tier=<tier>, reversible=<rev>, conf=<conf>, rationale=<rat>]`,
  and escalation transitions and verdicts.

## Non-Functional Requirements

- **NFR-1 — No regression.** All existing tests (currently 352) MUST continue to pass at every
  increment. Each increment builds clean (`cargo build`) and lints (`cargo clippy`).
- **NFR-2 — Prompt-injection resistance (threat).** The evaluator's minimal context + stricter-only
  clamp + non-pardonable policy floor MUST be the stated, tested mitigations against arguments or
  fetched content attempting to talk the evaluator into a low verdict. A low verdict can never
  unlock a policy-forbidden or over-ceiling action.
- **NFR-3 — Control-channel integrity (threat).** The inter-agent WebSocket channel MUST use
  mutual TLS with ephemeral fingerprint-pinned keys (no CA), channel-bound challenge–response,
  and loopback-only binding. A connection failing mutual auth is rejected; forged control messages
  are structurally impossible without completing the handshake. The durable rollback journal is
  separate and owner-only (`0600`).
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
