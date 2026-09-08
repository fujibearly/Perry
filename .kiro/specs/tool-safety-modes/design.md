# Tool Safety Modes & Actuation Governance — Design

## Approach

Implement the decision funnel as a set of composable gates evaluated **cheapest and most
deterministic first**, so that the LLM evaluator can never precede its own guardrail and the
fast-path stays free. Each increment adds one gate (or the channel that connects them) and is
wired so that, absent the richer gates, control falls through to the next-simplest behavior.

```
Action proposed by any agent (any depth)
   │
   ▼
[Protected Policy File]  deterministic, non-pardonable            ──(forbid)──► BLOCKED
   │                                                                            (policy_forbidden)
   ▼
Blast-radius classify {Safe…Catastrophic}  +  reversibility PROVEN? (real artifact)
   │      effective required authority = f(tier, proven_reversible)
   ├─ ≤ this agent's ceiling ───────────────────────────────────────────────► EXECUTE
   │        └─ plan-time flagged step? → mandatory act-time evaluator re-check
   ▼ (> ceiling, or uncertain, or Safe? → skip evaluator entirely)
[%assess-risk% role · dedicated model · minimal context]  STRICTER-ONLY
   │        clamp: may raise tier / withhold reversibility credit; may NOT loosen
   │        (fail / low-confidence ─► escalate)
   ▼
Escalate → INVOKING agent    (branch suspends; siblings run on)
   │   child writes escalation file (WHY + enrichment + proposed action), STAYS ALIVE polling
   │   parent merges enrichment into its context, decides, writes AUTHENTICATED verdict file:
   │        HALT (graceful) │ REVERT (child rolls back) │ CONTINUE (child resumes)
   │        unresponsive child → hard-kill
   ▼ (parent ceiling insufficient → escalate upward, accumulating evidence … → Orchestrator)
Orchestrator ceiling insufficient
   │
   ▼
HUMAN — blocking prompt on THIS branch via interactive CLI (siblings continue)
        │ OR │ emit same escalation record to a Layer 3 supervisor (headless/preferred)
```

The funnel is the *target* (#6d complete). Each increment realizes a prefix of it, with the
"escalate" arrow replaced by "block" until #6d lands.

## Data model

### `FunctionDeclaration` additions (`src/function.rs`)

Following the exact pattern that added `output: Option<OutputRouting>` — all skip-serialized so
the model never sees governance metadata:

```rust
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
    #[serde(skip_serializing, default)] pub agent: bool,
    #[serde(skip_serializing, default)] pub output: Option<OutputRouting>,

    // #6a
    #[serde(skip_serializing, default)] pub mode: Option<ToolMode>,       // readonly | mutating
    // #6b
    #[serde(skip_serializing, default)] pub risk: Option<BlastRadius>,    // Safe..Catastrophic
    #[serde(skip_serializing, default)] pub reversible: Option<bool>,     // intrinsic reversibility
    #[serde(skip_serializing, default)] pub reversible_via: Option<String>, // e.g. "backup", "worktree"
}
```

```rust
#[derive(…, Default)] #[serde(rename_all = "lowercase")]
pub enum ToolMode { Readonly, #[default] Mutating }
// Note: an *absent* `mode`/`risk` is "unclassified" — treated as reserved-to-humans
// (FR-6a.2/6b.3), which is stricter than `Mutating`. `Mutating` is the default only for a
// tool that positively declares a mode without a finer tier.

#[derive(…, PartialEq, Eq, PartialOrd, Ord)] #[serde(rename_all = "lowercase")]
pub enum BlastRadius { Safe, Reversible, Disruptive, Destructive, Catastrophic }
```

`BlastRadius` derives `Ord` so ceiling comparisons are a simple `<=`. Note `Reversible` appears as a
*tier name* for the low-impact-but-mutating band **and** reversibility is *also* a separate proven
boolean (FR-6b.2) — the tier is the impact axis, the boolean is the proof axis; they are combined only
when computing required authority.

### Effective required authority (the orthogonal combination)

```
required_authority(action) =
    if unclassified(action) { HUMAN }                                            // reserved-to-humans (for now)
    else {
        let base = max(static_tier(tool, args), policy_tier(policy_file, action)); // policy can only raise
        if proven_reversible(action) { one_step_down(base) } else { base }         // proof lowers requirement
    }
    // #6c: evaluator may raise `base` and may withhold proof; never the reverse
```

An **unclassified** action (tool declares no `mode`/`risk`, including MCP tools) yields a
`HUMAN` requirement that sits above every autonomous ceiling, so it always escalates to a human
(or blocks in increments before #6d). `proven_reversible` is true iff the tool declares intrinsic
reversibility **or** a rollback artifact is registered for this invocation (a backup/staged
copy/worktree recorded out-of-band — consumed from #9/#10).

### Config additions (new top-level `safety:` section in `src/config/mod.rs`)

The safety configuration is its **own top-level section** (a sibling of `agent_loop:`, not nested
under it), since it governs actuation policy across the whole tree rather than a single loop's
budget/observability knobs.

```rust
// top-level `safety:` — all serde(default), safe defaults (NFR-7):
pub struct SafetyConfig {
    pub policy_file: Option<PathBuf>,   // #6b Protected Policy File; None = built-in fail-safe defaults
    pub risk_model: Option<String>,     // #6c evaluator model; None = evaluator skipped (degrade to #6b)
    pub default_ceiling: BlastRadius,    // top-level ceiling; default e.g. Destructive (human for Catastrophic)
    pub escalation_dir: Option<PathBuf>, // #6d; default $XDG_RUNTIME_DIR
    pub verdict_timeout_secs: u64,       // #6d poll timeout before further escalation / hard-kill
}
```

Defaults are the safe choice (NFR-7): no policy file → built-in fail-safe classification; no risk model →
deterministic-only; ceiling defaults leave `Catastrophic` to humans.

### Environment propagation (child spawn, `eval_agent_tool_subprocess`)

Rides the existing `AICHAT_AGENT_DEPTH` channel:

| Env var | Increment | Meaning |
|---------|-----------|---------|
| `AICHAT_CAPABILITY_MASK` | #6a | `readonly` restricts child to `Safe`/`readonly` tools |
| `AICHAT_AUTHORITY_CEILING` | #6b | max `BlastRadius` the child may act on autonomously (parent may only lower) |
| `AICHAT_AGENT_PARENT_ADDR` | #6d | loopback WSS address + port the child dials back to connect to the parent |
| `AICHAT_AGENT_TOKEN` | #6d | per-child credential seed for the mTLS mutual-auth handshake |
| `AICHAT_TREE_SECRET` | #6d | per-tree root-of-trust for ephemeral key derivation (never written to disk) |

## Seam Map

| Requirement | Seam / function | Location | Technique |
|-------------|-----------------|----------|-----------|
| FR-6a.1/6a.2 | `FunctionDeclaration.mode` + deserialize default | `function.rs` | Add field; `ToolMode::default()==Mutating`; unit-test JSON round-trip + default |
| FR-6a.3/6a.5 | mask env set on child | `agent_loop.rs::eval_agent_tool_subprocess` | Set `AICHAT_CAPABILITY_MASK=readonly`; top-level (depth 0) unmasked |
| FR-6a.4 | capability gate before dispatch | `agent_loop.rs::eval_single_tool` (pre-Route) | If masked && tool mutating → return `capability_denied` result (mirror tripped-tool partition) |
| FR-6b.1/6b.2/6b.3 | `BlastRadius`, `reversible*` fields | `function.rs` | Enum w/ `Ord`; map legacy `mode`; unit tests for ordering + mapping |
| FR-6b.4 | Protected Policy File loader + matcher | new `src/safety.rs` | Load owner-only file; `policy_tier(action) -> BlastRadius | Forbidden`; can only raise |
| FR-6b.5/6b.6 | ceiling compare + env propagation | `agent_loop.rs` | `required_authority(action) <= ceiling` else `authority_exceeded`; child ceiling = min(self, granted) |
| FR-6b.7 | escalation/verdict message schema (reserved) | `src/safety.rs` | `struct EscalationMsg { …, agent_id, tree_id, challenge/nonce }` + `VerdictMsg` defined now (typed WS messages, unused until #6d) |
| FR-6c.1 | `%assess-risk%` role asset | `assets/roles/%assess-risk%.md` | Terse structured-verdict prompt, `%explain-shell%` shape |
| FR-6c.2 | evaluator invocation w/ dedicated model | `src/safety.rs` + client | Build a minimal `Input` with `safety.risk_model`; parse JSON verdict |
| FR-6c.3 | minimal-context builder | `src/safety.rs` | Only {tool, resolved args, static tier, reversibility, this-step intent}; explicitly exclude history |
| FR-6c.4 | verdict struct + parse | `src/safety.rs` | `RiskVerdict { tier, reversible, confidence, rationale, concerns, enrichment }`; tolerate malformed → low-confidence |
| FR-6c.5 | stricter-only clamp | `src/safety.rs` | `effective = max(static, verdict.tier)`; proof only removed, never added; policy untouched. **Pure fn, unit-tested** |
| FR-6c.6 | fast-path | `agent_loop.rs` | `if tier == Safe { skip evaluator }` |
| FR-6c.7 | plan-time flagging + act-time recheck | `agent_loop.rs::run` (plan partition) | Plan pass tags steps; only tagged steps re-evaluated at dispatch |
| FR-6c.8 | fail-toward | `src/safety.rs` | evaluator error/low-confidence → escalate (or block pre-#6d) |
| FR-6d.2/6d.3 | WSS listener + mTLS handshake | `src/safety.rs` + `agent_loop.rs` | Parent binds loopback WSS; child dials back; mutual auth via ephemeral pinned keys + channel-bound challenge–response; reject unverified connections |
| FR-6d.4 | message protocol + escalation flow | `src/safety.rs` + `agent_loop.rs` | Typed messages (Hello/Event/Escalation/Result upstream; Verdict/Cancel downstream) over the WSS connection; child blocks on `recv()` (no polling) |
| FR-6d.5 | HALT/REVERT/CONTINUE handling | `agent_loop.rs` (child's connection handler) | Child awaits Verdict on open socket, dispatches verb; REVERT replays durable on-disk journal entry |
| FR-6d.6 | durable rollback journal | `src/safety.rs` | Append-only on-disk journal (separate from the control channel) — reversibility survives connection drops / child death |
| FR-6d.7 | upward propagation | `agent_loop.rs` | Parent that can't decide re-escalates up its own connection to its parent, appending to evidence trace |
| FR-6d.8 | human sink | `agent_loop.rs` + `main.rs`/`repl` | Interactive: branch-blocking prompt (siblings run). Headless: emit record to Layer 3 |
| FR-6d.9 | liveness | (implicit from connection state) | Parent/child death → immediate EOF on the other side; replaces `/proc/<pid>` scanning |
| FR-6d.10 | graceful/hard stop | `agent_loop.rs` | Cooperative Cancel/HALT via the connection; signal-kill on verdict timeout |
| FR-6d.8 | branch-scoped suspension | `agent_loop.rs` | Suspension is per-lineage future; `join_all` siblings unaffected |

## Key Design Decisions

- **New module `src/safety.rs`.** Classification, policy loading, the evaluator invocation +
  stricter-only clamp, and the escalation/verdict message types live in one cohesive module with a
  small, pure, heavily-unit-tested core (`required_authority`, `clamp_verdict`, `policy_tier`).
  `agent_loop.rs` calls into it; `function.rs` only holds the metadata fields.
- **Metadata is skip-serialized.** Governance is invisible to the LLM (consistent with `output`),
  so the model can't reason about — or be manipulated through — its own guardrails.
- **Fail-safe defaults everywhere.** *Unclassified* tool (no `mode`/`risk`, incl. MCP) → reserved
  to humans (for now); a tool that declares a mode but no finer tier → `Mutating`; missing policy
  file → built-in conservative rules; evaluator absent/failed → deterministic block/escalate;
  `Catastrophic` reserved to humans by default. Safety is the default, capability is opt-in.
- **Ordering enum for cheap comparisons.** `BlastRadius: Ord` makes ceiling checks and the
  stricter-only clamp trivial `max`/`<=` operations that are obviously correct and unit-testable.
- **Reversibility is proof-gated and orthogonal.** It is *not* a tier; it is a separate boolean that
  only counts with a real artifact and only ever *reduces* the authority required — never the radius.
- **Escalation is a live control relationship over a still-running child, via a mutually-authenticated
  WebSocket.** The parent binds a loopback WSS listener; the child dials back after spawn. Chosen
  over files (eliminates polling latency and liveness-scanning overhead), over stdin/stdout (couples
  to the child's I/O, fragile under the existing stdout-capture paths), and over kill-only (can't
  express REVERT/CONTINUE). The connection gives free bidirectional messaging and free liveness
  detection (EOF on death). ARGC is explicitly not used.
- **Mutual TLS with ephemeral fingerprint-pinned keys, no CA/PKI.** Both sides prove identity at
  connection time. The parent generates an ephemeral keypair per agent tree; the child receives the
  parent's public-key fingerprint + a per-child credential via its private spawn env. The handshake
  is channel-bound so leaked credentials cannot be replayed on another connection. Loopback-only
  binding as defense-in-depth.
- **The control channel is NOT the durability plane.** The WebSocket is ephemeral — it dies with the
  connection. Reversibility is backed by a **durable on-disk rollback journal** (append-only, per
  agent, written before/at mutation). REVERT replays a journal entry, not in-memory state, so
  reversal survives connection drops, child crashes, and re-spawns. The control channel carries
  the *verdict to revert*; the journal carries the *recipe for how*.
- **Transport-independent message protocol.** Messages are typed data (Hello, Event, Escalation,
  Result, Verdict, Cancel) defined independently of the transport. The same protocol generalizes
  to remote agents (WSS over a routable interface with real certs) without a redesign — this is a
  stated forward-compatibility requirement (FR-6d.12).
- **The evaluator is a role, not hardcoded.** `%assess-risk%` is a user-editable asset like
  `%explain-shell%`, keeping prompt logic in the declarative layer and the *enforcement* (clamp,
  fail-toward, fast-path) in Rust where it must be trustworthy.

## As-Built Notes — #6b decisions made during implementation

These refine the spec above based on decisions taken while implementing and validating #6b
against the live demo harness. They are authoritative for the #6b as-shipped behavior.

- **Tools are classified rather than the engine loosened.** The spec's "unclassified →
  human-reserved" rule is kept strict. Rather than default unclassified tools to something
  permissive, all 31 stock `llm-functions` tools were classified via `# @meta risk <tier>`
  (+ `# @meta reversible true` where trivially undone), and `build-declarations.{sh,js,py}`
  were extended to emit `risk`/`reversible`/`reversible_via` into `functions.json`
  (sh: argc `.metadata`; js: JSDoc `@meta`; py: a `Meta:` docstring block). This lives in the
  **`llm-functions` repo** (branch `feat/tool-safety-classification`), not this repo. Net
  effect: "unclassified → human" now fires only for genuinely unknown tools (e.g. MCP), which
  is the intended safety posture.
- **Decision B — delegation is not gated.** A tool call that targets a sub-agent
  (`call_targets_agent`: an `agent`-flagged function naming a real agent) skips *both* the #6a
  capability gate and the #6b authority gate. Delegating is orchestration, not actuation; the
  sub-agent's *own* actions are gated inside its process via the inherited capability mask +
  authority ceiling. Without this, unclassified agent-tools would be human-reserved and
  multi-agent mode would be off by default — double-counting the risk. (Chosen over: classifying
  agents themselves, or accepting delegation as human-reserved.)
- **Catastrophic is a hard human-only floor.** `required_authority` does NOT apply the
  proven-reversibility one-step discount when the base tier is `Catastrophic` (guard:
  `base != Catastrophic`). This prevents a tool-author-set `reversible: true` from silently
  undercutting a policy-imposed `catastrophic` raise. Catastrophic always exceeds any
  autonomous ceiling → human (blocks pre-#6d). Reversibility still discounts the lower tiers.
- **`ToolBlocked` trace event.** A gate denial returns via the dispatcher's `Ok` path (so the
  model sees the structured refusal), which previously made the loop trace print
  `<tool> completed`. A distinct `AgentLoopEvent::ToolBlocked { name, reason }` now fires for
  the three gate reasons (`capability_denied` / `authority_exceeded` / `policy_forbidden`, via
  `safety_block_reason`), so the trace reads `<tool> BLOCKED (<reason>)`, output routing is
  skipped, and the trace never implies the tool ran.
- **Config env overrides.** `AICHAT_SAFETY_POLICY_FILE` and `AICHAT_SAFETY_DEFAULT_CEILING`
  were added to `config::load_envs` (matching the `AICHAT_AGENT_LOOP_*` pattern) so policy and
  ceiling can be set for scripting/ops (and the demo harness) without editing `config.yaml`.
- **Live validation.** Demos 13 (policy `forbid`), 14 (authority-ceiling over-run), and 15
  (argument-sensitive `raise` to catastrophic on `execute_command` matching `rm -rf`) exercise
  the gate end-to-end on a cheap model; see `scripts/run-demos.nu`.

## As-Built Notes — #6c decisions made during implementation

These refine the #6c design based on decisions taken while implementing it. They are
authoritative for the #6c as-shipped behavior.

- **The evaluator is act-time, not plan-time; "two-phase" became a raise-only cache.** The spec
  proposed a plan-time pass that flags key steps, with only flagged steps re-checked at act time
  (policy floor not re-checked). Implementation revealed two problems: (1) the agent loop is
  turn-based ReAct — the `_plan` tool is a free-text scratchpad (`{thought: string}`), not a
  structured list of upcoming steps, so there is nothing concrete to flag against; and (2) more
  fundamentally, any design where an *unflagged* step skips its act-time check is a hole exactly
  where prompt-injection attacks aim (get the model to under-declare intent). So the literal
  two-phase model was **superseded**. Instead:
  - **Act-time evaluation is the non-negotiable floor.** Every non-`Safe`, in-ceiling action is
    evaluated by `%assess-risk%` (or served from cache) immediately before it runs.
  - **A monotonic, raise-only `RiskCache`** (in `safety.rs`, keyed by tool + canonical resolved
    args, one per `run`, shared across turns) delivers the cost win the two-phase design sought:
    an identical action assessed once is not re-evaluated. Crucially the cache is **raise-only**
    (`stricter_of`) — it can reuse a recorded authority *floor* to skip a model call, but can
    never lower one, so it can pre-raise (stop earlier) but never pre-clear (green-light).
  - **Assessment is a one-way ratchet toward "stop."** A user-framed principle adopted here: a
    risk assessment (act-time now, or a future whole-plan pre-pass) is valuable as an *earlier,
    cheaper red-light* with potentially better context — never as a green-light. The clamp and the
    raise-only cache enforce this structurally.
  - **The serious structured `_plan` is deferred to its own backlog item.** A real plan-driven
    executor (structured steps, plan-driven execution) would enable a genuine whole-plan red-light
    *pre-pass* — evaluating declared steps with full cross-step context and pre-raising their cache
    floors before the agent walks toward them. That is a larger change to the loop contract than a
    safety increment should carry, so it is a separate item; when it lands it writes into the same
    `RiskCache` and remains strictly raise-only. The act-time floor is unchanged by its presence
    or absence.
- **Low confidence caches as `Human`.** A low-confidence / errored verdict fails toward blocking
  (FR-6c.8). To keep a later identical action consistent without re-calling the model, that
  outcome is recorded in the cache as a `Human` floor (the strictest), so reuse re-blocks
  deterministically.
- **Dedicated model via `set_model`.** The evaluator retrieves the `%assess-risk%` role, then
  overrides the model with `safety.risk_model` (`Model::retrieve_model` + `role.set_model`) rather
  than relying on role front-matter, so operators configure the evaluator model in one place.
- **Pipe-target actuations are gated fresh.** A tool executed as an output-routing pipe target
  runs through the same gate but with no shared `RiskCache` (it is a derived call outside the turn
  loop) — still fully gated, just evaluated fresh rather than cache-reused.
- **Enriched Semantic & Implementation Context (Ending Black-Box Evaluation).**
  - *Motivation:* Blind-spot evaluation where the evaluator model only sees opaque tool names and
    arguments (e.g. `execute_command` with `"git status"`) forced the model to guess whether the underlying
    tool uses `eval`, sanitizes inputs, ignores flags, or executes hardcoded side effects.
  - *Architecture:*
    1. **`ToolDeclarationContext` ([`src/safety.rs`](../../src/safety.rs)):** Merges JSON schema descriptions
       with docstrings parsed directly from tool script headers via `parse_script_header_comments()`.
       Extracts `# @describe`, multi-line documentation comments, parameter schemas, and `# @env` variables.
    2. **Dynamic Tool Resolution (`resolve_tool_implementation` in [`src/agent_loop.rs`](../../src/agent_loop.rs)):**
       Scans MCP tools, agent `tools/` and `bin/`, and root functions `tools/` and `bin/`. Recursively traverses
       runner symlinks (e.g. `bin/execute_command -> ../scripts/run-tool.sh -> tools/execute_command.sh`), detects
       binary executables via null-byte inspection across the first 512 bytes, and captures script source with a
       strict 4KB text budget sliced at valid UTF-8 grapheme/char boundaries.
    3. **Invocation Preview (`format_tool_invocation` in [`src/agent_loop.rs`](../../src/agent_loop.rs)):**
       Renders the exact command string with bound CLI arguments (e.g. `execute_command --command "git status"`).
    4. **Evaluator Role Alignment ([`assets/roles/%assess-risk%.md`](../../assets/roles/%25assess-risk%25.md)):**
       Instructs the model to contrast declared `@describe` intent against the implementation source, trace argument
       flow into sinks (`eval`, `rm`, shell interpreters, curl), detect hardcoded destructive operations, and check
       for confirmation guards (e.g. `guard_operation.sh`).
    5. **Security Invariant:** Scratchpad and conversation history remain excluded to bound prompt-injection
       exposure. Arguments are tagged as untrusted data. The engine-level `clamp_verdict` guarantees the model
       can only raise risk or withhold reversibility credit — the LLM is an observer and red-light raiser, never a pardoner.


## As-Built Notes — #6d transport decision (Path 1′)

Authoritative for the #6d as-shipped transport. The escalation channel's **security model and
message protocol are exactly as designed above** (mTLS, ephemeral in-memory per-tree keypair,
fingerprint pinning, channel-bound challenge–response, loopback-only; typed
`Hello`/`Event`/`Escalation`/`Result`/`Verdict`/`Cancel`). Only the **wire framing** changed.

- **mTLS over a raw loopback TCP stream + hand-rolled length-delimited JSON framing — not
  WebSocket.** Frames are a 4-byte big-endian length prefix followed by `serde_json` bytes, read/
  written over `tokio_rustls::TlsStream`. WebSocket framing (masking, opcodes, ping/pong, close
  handshake) is browser/HTTP-proxy machinery that a parent↔child loopback channel does not need.
- **Why not `tokio-tungstenite`.** Adding it would double the fork's dependency additions for a
  framing layer we don't use; the user's explicit dependency-brittleness concern tipped this. TLS —
  the security-critical and remote-valuable part — is built now and reuses the `tokio-rustls`
  already in the tree (via reqwest). The only new crate is `rcgen` (+ its tiny `yasna` transitive)
  for ephemeral self-signed cert generation; `rustls`/`ring`/`rustls-pki-types` are reused with no
  second TLS backend and no OpenSSL (bastion-friendly).
- **`EscalationTransport` trait seam preserves the remote goal.** The transport is abstracted behind
  a trait carrying the typed messages + the auth handshake contract; the loopback-TLS impl is the
  only concrete impl now. A **WebSocket-over-routable-TLS** transport can be added as a second impl
  *when a remote deployment (proxy/browser in the path) actually needs it* — no protocol or auth
  redesign, satisfying FR-6d.12. This is a deliberate deferral, not a drop: the hard part (mutual
  TLS) ships now; only the (browser-oriented) framing is deferred until it has a real consumer.
- **Custom rustls verifiers are the security-critical core.** Fingerprint pinning is implemented via
  `ClientConfig`/`ServerConfig` `.dangerous().with_custom_certificate_verifier(...)`. Getting this
  right (fail-closed on any error, bind the challenge to the TLS session, reject on fingerprint
  mismatch or replay) is where the care and the most thorough tests go.
- Wherever the design/requirements above say "WSS"/"WebSocket", read "mutual-TLS loopback stream
  (WS framing deferred behind the transport trait)".

## Threat Model (explicit, per NFR-2/3/4)

1. **Prompt injection into the evaluator** — a tool's arguments or fetched content tries to coerce a
   low verdict. Mitigations: minimal context (no room to hide instructions that matter), stricter-only
   clamp (a low verdict *cannot* unlock anything the deterministic layer didn't already allow), and the
   non-pardonable policy floor (forbidden stays forbidden without ever consulting the LLM).
2. **Forged control messages / unauthorized connection** — a local tool, injected content, or
   compromised peer attempts to connect to the parent's WSS listener and forge a CONTINUE verdict
   or inject a fake escalation. Mitigation: mutual TLS with ephemeral fingerprint-pinned keys;
   channel-bound challenge–response so leaked credentials are non-replayable; loopback-only binding
   so off-box connections are physically impossible; connection failing handshake is rejected before
   any message is exchanged.
3. **Cost/latency** — an evaluator call before every mutation. Mitigation: `Safe` fast-path skips the
   evaluator entirely; plan-time pass bounds calls and flags only the steps needing an act-time recheck.
4. **Evaluator as single point of trust** — mitigated structurally: it is advisory and stricter-only;
   the deterministic floor and human escalation are the real authority.

## Verification

- Pure-core unit tests (in `src/safety.rs`): tier ordering; legacy-mode mapping; `required_authority`
  with/without proven reversibility; `policy_tier` raise-only; `clamp_verdict` stricter-only (a
  permissive verdict is a no-op; a stricter one raises).
- `agent_loop.rs` tests: masked child denies a mutating tool (`capability_denied`); over-ceiling →
  `authority_exceeded`; `Safe` fast-path skips evaluator (assert no evaluator call via a mock seam);
  escalation `Escalation`/`Verdict` messages round-trip over the WebSocket channel with a mock parent
  verdict; HALT/REVERT/CONTINUE dispatch; sibling parallelism unaffected by a suspended branch.
- Evaluator tested with a **mock verdict** (deterministic), never a live model.
- Full `cargo test` green at the end of each phase (NFR-1); `cargo clippy` clean.
- Because `eval_agent_tool_subprocess` spawns `current_exe()` (the test binary under `cargo test`),
  full parent↔child escalation over real subprocesses is validated by an **offline demo** in
  `scripts/run-demos.nu` (à la Demo 12), not a unit test.
