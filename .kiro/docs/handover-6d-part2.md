# Handover — Backlog #6d Part 2 (Escalation loop integration)

**Written:** 2026-09-02. **For:** whichever agent continues #6d.
**Repo:** `/home/istari/projects/aichat` · **Branch:** `feat/tool-safety-6d` · **HEAD:** `d40bccc`
**Version:** v0.31.0-fork.9 · **Tests at handover:** 432 workspace (424 unit + 5 + 3), 0 fail.

---

## 0. Read first

- Spec (source of truth): [`.kiro/specs/tool-safety-modes/`](../specs/tool-safety-modes/) —
  `requirements.md` (FR-6d.1–6d.12), `design.md` (funnel + "As-Built Notes — #6d transport
  decision (Path 1′)"), `tasks.md` (Phase #6d checklist 6d.1–6d.13).
- Consolidated status: [`roadmap.md`](roadmap.md) (canonical Status Table).
- The umbrella: #6a (capability mask), #6b (tiers/reversibility/policy/ceiling), #6c
  (`%assess-risk%` LLM evaluator) are **done + committed** on their stacked branches. #6d is the
  last increment; **part 1 (transport + auth) is committed at `d40bccc`**.

## 1. Standing user decisions (do NOT re-litigate)

- **Path 1′ transport:** mutual-TLS over a **raw loopback TCP stream + length-delimited JSON
  framing**. **NOT WebSocket** — WS framing is deferred behind the `EscalationTransport` trait until
  a remote deployment (proxy/browser) needs it. TLS (the remote-valuable, security-critical part)
  is built now.
- **Dependency discipline is a hard value.** Fork has added only `async-stream`, `rcgen` (+ tiny
  `yasna`). Do NOT add `tokio-tungstenite` or any TLS backend. `rustls`/`tokio-rustls` are reused
  (single version, no OpenSSL).
- **No literal static token.** Child auth is a **channel-bound HMAC** (`HMAC(tree_secret,
  nonce | parent_fingerprint)`), stronger than a token (non-replayable). Keep it this way.
- **Local-only; ask before committing.** User approves commits explicitly. Nothing is pushed.
- Nushell shell: wrap pipelines in `bash -c "..."`; `&&`/`2>&1` don't work bare. Tests:
  `cargo test --bin aichat` (binary crate). Release build is slow (~5–9 min) — use debug.
- Standing safety invariants from #6a–#6c still hold: deterministic floor first; the LLM is not a
  Pardoner; **Catastrophic = always human**; delegation is orchestration (not gated).

## 2. What part 1 delivered (committed `d40bccc`) — DONE, do not redo

All in `src/escalation.rs` (+ protocol types in `src/safety.rs`, `sha256_bytes` in
`src/utils/crypto.rs`, module registered in `src/main.rs`, deps in `Cargo.toml`).

**Protocol (`safety.rs`):** `HelloMsg`, `EscalationMsg`, `ResultMsg`, `ErrorMsg` (upstream);
`VerdictMsg`, `CancelMsg`, `VerdictDecision{Halt,Revert,Continue}` (downstream). Unified in tagged
envelopes `UpstreamMsg` / `DownstreamMsg` (`#[serde(tag="type", rename_all="snake_case")]`).
`Event(serde_json::Value)` carries loop trace opaquely.

**Framing (`escalation.rs`):** `write_frame`/`read_frame` — 4-byte BE length prefix + `serde_json`
over any `AsyncRead/Write`. `read_frame` → `Ok(None)` on clean EOF at a boundary (this is the
liveness signal, FR-6d.9), error on truncation, rejects `len > MAX_FRAME_BYTES` (8 MiB).

**Auth core (`escalation.rs`) — the security-critical part, fully tested:**
- `TreeIdentity{cert_der,key_der,fingerprint}` + `generate_tree_identity()` — ephemeral in-memory
  self-signed cert via `rcgen` 0.13 (`CertifiedKey{cert,key_pair}`; `cert.der()`;
  `key_pair.serialize_der()`); `fingerprint = sha256_bytes(cert_der)`.
- `FingerprintPinVerifier` impls `rustls::client::danger::ServerCertVerifier` — accepts iff
  presented cert's SHA-256 == pinned, else `Err` (**FAIL CLOSED**); sig-verify delegates to
  `rustls::crypto::ring::default_provider()`.
- `compute_channel_credential(tree_secret, nonce, parent_fp)` = `hex(HMAC-SHA256(secret,
  "{nonce}|{fp}"))`; `verify_channel_credential(...)` uses `constant_time_eq`. Channel-bound:
  fingerprint binding defeats cross-parent replay; nonce = per-connection.

**Transport + handshake (`escalation.rs`):**
- `EscalationTransport` trait (async): `send/recv_upstream`, `send/recv_downstream`.
- `TlsTransport<S>` — concrete impl over a `tokio_rustls` stream (server or client side).
- `build_server_config(identity)` / `build_client_config(pinned_fp)` — rustls 0.23 with an
  **explicit ring provider** (`builder_with_provider`), so no dependence on a global
  `CryptoProvider` install. Server: `with_no_client_auth` + `with_single_cert`. Client:
  `.dangerous().with_custom_certificate_verifier(FingerprintPinVerifier)`.
- Pre-protocol handshake frames: `ChallengeFrame{nonce}` (parent→child), `CredentialFrame{agent_id,
  credential}` (child→parent), sent BEFORE the typed protocol.
- `ParentListener::bind(identity, tree_id, tree_secret)` — binds **`127.0.0.1:0` loopback only**.
  `.local_addr()`, `.fingerprint()`, `.child_env()` → the 4 env vars, `.accept_authenticated()` →
  `(TlsTransport<server>, HelloMsg)` after TLS + nonce + credential-verify (drops on fail) + Hello.
- `ParentConnInfo::from_env()` → `Option` (None = top-level or #6d disabled → degrade to pre-#6d).
  `dial_parent(info, agent_id, depth)` → `TlsTransport<client>` after pinned TLS + challenge +
  credential + Hello.

**Env vars (FINALIZED):** `AICHAT_AGENT_PARENT_ADDR`, `AICHAT_AGENT_PARENT_FP` (parent cert
fingerprint the child pins), `AICHAT_TREE_SECRET` (shared credential seed), `AICHAT_TREE_ID`.
(Spec originally said `AICHAT_AGENT_TOKEN`; superseded by the channel-bound HMAC model — document in
6d.13, don't add a token.)

**Tests (14, all green):** framing (4), envelope round-trips (3, in safety.rs), auth primitives
(fingerprint/credential/constant-time/verifier fail-closed), and **3 end-to-end localhost mTLS
handshake tests** (legit authenticates; wrong fingerprint rejected by child; wrong tree-secret
rejected by parent).

**Note:** `escalation.rs` still has a module-level `#![allow(dead_code)]` — remove it once the
transport is wired into `agent_loop.rs` (part 2), then fix any genuinely-unused items.

## 3. What remains — Part 2 (tasks.md 6d.4–6d.13)

Recommended order (dependency-first):

1. **6d.6 — Durable rollback journal** (`escalation.rs` or a small `journal` submodule).
   Append-only on-disk JSONL under an isolated dir (respect `safety.escalation_dir`, default
   `$XDG_RUNTIME_DIR`). Entry: `{action, artifact_path, undo_command, agent_id, timestamp}`. Write
   BEFORE/at a proven-reversible mutation. `REVERT` replays the entry. Must survive connection drop
   / child death / re-spawn (that's the whole point — it's the durability plane, separate from the
   ephemeral control channel). Unit-test replay + survives-reopen. (FR-6d.6)
2. **6d.4 — Child escalation trigger + handler.** This is the invasive one. Today, when the #6b
   authority gate or #6c evaluator would block an over-ceiling/hesitant action, the dispatcher
   returns a structured `authority_exceeded`/`risk_blocked`/`policy_forbidden` result (see
   `agent_loop.rs::authority_denied_result` / `risk_evaluator_denied_result` / `eval_single_tool`).
   For #6d: if `ParentConnInfo::from_env().is_some()` (we're a child with a live parent), instead of
   returning the block, **dial the parent (or reuse a per-process connection), send `Escalation`,
   block on `recv()` for a `Verdict` up to `safety.verdict_timeout_secs`**, then act on the verdict.
   If no parent conn (top-level) → keep today's block behavior (this IS the degrade path, FR + NFR-6).
   Keep the connection per-process (dial once, reuse) — a `tokio::sync::Mutex<Option<transport>>` in
   a process-global or threaded through. (FR-6d.1/6d.4)
3. **6d.5 — Verdict verbs in the child.** HALT → return a structured "halted" result, do not run the
   tool. REVERT → replay the journal entry, return "reverted". CONTINUE → run the tool (optionally
   with `added_context`). Unit-test dispatch of each with a mock transport (use `tokio::io::duplex`
   or a mock `EscalationTransport`). (FR-6d.5)
4. **Env-passing wiring** in `agent_loop.rs::eval_agent_tool_subprocess` (~line 690+, where
   `AICHAT_CAPABILITY_MASK`/`AICHAT_AUTHORITY_CEILING`/`AICHAT_AGENT_DEPTH` are set on the child
   `Command`): the spawning parent must (a) lazily create/hold a `ParentListener` + `TreeIdentity`
   for its tree, (b) `cmd.envs(listener.child_env())`, (c) spawn an accept task that handles the
   child's connection (the parent side of the escalation — receives `Escalation`, decides or
   re-escalates, sends `Verdict`). The tree identity/secret should be generated once at the
   orchestrator and propagated (the child re-exports the SAME `AICHAT_TREE_*` to ITS children so the
   whole tree shares one trust root — every agent is both listener for its children and client to
   its parent, FR-6d.7).
5. **6d.7 — Parent decision / upward propagation.** Parent merges the child's `enrichment`, applies
   its OWN ceiling/evaluator; if within its authority it decides (Halt/Revert/Continue); else it
   re-escalates up ITS parent connection, accumulating the evidence trace, until the orchestrator.
   (FR-6d.7)
6. **6d.8 — Human-in-the-loop at the orchestrator.** If the orchestrator can't decide: interactive
   branch-blocking CLI prompt (use `inquire`, already a dep) showing action/tiers/evidence →
   approve/deny/revert; OR emit the same escalation record to a "Layer 3" sink when headless
   (`is_terminal`/config). One record, two sinks. (FR-6d.8)
7. **6d.9 — Liveness.** Already mostly free: `read_frame` → `Ok(None)` on EOF = peer died. Wire it so
   a child whose parent died aborts, and a parent whose child died stops waiting. Hard-kill an
   unresponsive child on `verdict_timeout_secs` (the child subprocess handle is in
   `eval_agent_tool_subprocess`). (FR-6d.9/6d.10)
8. **6d.10 — Branch-scoped suspension.** A lineage blocked awaiting a verdict must not block sibling
   `join_all` in `eval_tool_calls_parallel`. Since each tool future is already independent and the
   escalation `recv()` is `.await`, this should hold naturally — add an async test with two
   concurrent tools where one escalates and blocks, asserting the other completes. (FR-6d.11)
9. **6d.11 — Offline demo** in `scripts/run-demos.nu` (à la Demo 12, which spawns real processes):
   real parent↔child escalation over loopback mTLS between spawned `aichat` processes — assert
   wrong-fingerprint rejection, verdict verbs, journal REVERT survives a killed child, branch-only
   suspension. (Cost-conscious: use `gemini-2.5-flash` `DEMO_MODEL` if any LLM call is needed, or
   keep it offline like Demo 12.)
10. **6d.12 — `cargo test` + `cargo clippy` green; degrade check:** with no `AICHAT_AGENT_PARENT_*`
    env (top-level, or #6d disabled) behavior is EXACTLY #6b/#6c (over-ceiling/hesitant → block, not
    escalate). Confirm all #6a/#6b/#6c gate tests still pass unchanged. (NFR-1/6)
11. **6d.13 — Docs:** architecture.md #6d section (full funnel, mTLS channel topology/auth/protocol/
    control-vs-durability-vs-audit planes, WS-deferred-behind-trait rationale, human/Layer-3 paths,
    threat model, remote hook); reconcile the `AICHAT_AGENT_TOKEN`→channel-bound-HMAC env naming;
    roadmap.md status table #6d → implemented + umbrella #6 → all four done; tasks.md 6d boxes;
    a Session 6 summary + SESSION_SUMMARY.md index entry.

## 4. Key integration points (exact locations)

- **`src/agent_loop.rs`:**
  - `eval_single_tool(config, call, risk_cache)` (~line 782) — the dispatch gate chain:
    `capability_denied_result` → `authority_denied_result` → `risk_evaluator_denied_result` → routes.
    **The escalation trigger replaces "return the block" when a parent conn exists.**
  - `authority_denied_result` (~line 501) and `risk_evaluator_denied_result` (~line 650) build the
    structured denials — these are where an over-ceiling/hesitant decision is currently detected.
  - `eval_agent_tool_subprocess` (~line 690+) — where the child `Command` env is set; the parent
    listener + `child_env()` wiring goes here.
  - `safety_block_reason` (~line 575) recognizes `capability_denied|authority_exceeded|
    policy_forbidden|risk_blocked` → emits `ToolBlocked`. Escalation outcomes may want their own
    trace events (`Escalated`, `Halted`, `Reverted`).
- **`src/config/mod.rs`:** `SafetyConfig` (~line 320) already has `escalation_dir` and
  `verdict_timeout_secs` fields reserved for #6d — use them (verify they exist; add if not).
- **`src/safety.rs`:** protocol types live here; `RequiredAuthority`/`AuthorityCeiling`/
  `RiskCache` are the decision core.

## 5. Design guardrails for part 2 (from the spec + Core Principles)

- **Fail toward escalation, not action.** No parent / timeout / dead connection → block, never
  silently permit.
- **Escalation suspends only the branch.** Siblings keep running.
- **The channel is control + telemetry only** — never a data plane. Tool outputs/artifacts stay
  isolated (Pillars 1/2) or move via #4/#13. REVERT replays the durable journal, NOT in-memory state
  or channel history.
- **Catastrophic still = human** regardless of any verdict (the #6b hard floor is upstream of #6d).
- **Degrade check is mandatory** at the end (NFR-6): #6d absent ⇒ exactly #6b/#6c.

## 6. Commit boundaries so far (feat/tool-safety-6d off feat/tool-safety-6c)

- `d40bccc` — #6d part 1: transport + auth core (this handover's subject).
- Parent chain: `58af926` (#6c) → `e530f95` (doc consolidation) → `a3e8eb6`/`7de3291` (#6b) →
  `8994922` (#6a). Nothing pushed (local-only, intentional).
