# Session Summaries & Handoff Index

This repository maintains continuous, chronological session handoff summaries documenting all architectural decisions, code changes, and test coverage.

**Key references:** [`.kiro/docs/roadmap.md`](.kiro/docs/roadmap.md) — the consolidated single source of truth with three aligned views: **Roadmap** (strategy / 4-layer crosswalk), **Traction** (current state + canonical status table), and **Backlog** (per-item detail). (Supersedes the former separate `backlog.md` + `progress.md`, now merged in.)

---

### Chronological Session Records:

1. **Session 1: Architectural Discovery, Taxonomy, SRE Landscape & Native RAG Upgrade**
   * **Period:** `2026-08-26T15:14:26-04:00` $\rightarrow$ `2026-08-28T16:47:33-04:00`
   * **Handoff Document:** [`.kiro/docs/session-summary-2026-08-26-to-2026-08-28.md`](.kiro/docs/session-summary-2026-08-26-to-2026-08-28.md)
   * **Focus Areas:**
     * 4-Layer Taxonomy & System Inventory (Roles, Agents, 31 Tools).
     * SRE Supervisory Landscape Evaluation (`dot-agent-deck` vs `bohay` vs 20MB Bastion Stack).
     * The 6 Unix Pillars of the Fork & Legacy OS Support.
     * Native Rust MCP Engine Review (`src/mcp.rs`).
     * `models-override.yaml` RAG loader fix in `src/config/mod.rs`.
     * Upgraded to Google's latest **`gemini-embedding-2`** and built session RAG `agy-aichat-catch-up`.

2. **Session 2: Repository Alignment, LLVM Code Coverage & Backlog Formalization**
   * **Period:** `2026-08-28T16:47:33-04:00` $\rightarrow$ `2026-09-02T10:57:37-04:00`
   * **Handoff Document:** [`.kiro/docs/session-summary-2026-08-28-to-2026-09-02.md`](.kiro/docs/session-summary-2026-08-28-to-2026-09-02.md)
   * **Focus Areas:**
     * Cleaned & committed pending changes in `aichat` (`fb5e011`) and `llm-functions` (`acbeb17`).
     * LLVM Dynamic Code Coverage benchmarking (**72.9% line / 79.4% function** on `src/agent_loop.rs`).
     * Formalized Backlog items 5–10 (Tool safety modes, WAL session resumption, rolling compaction, ephemeral Git worktrees).
     * Agent loop Mermaid operational diagram (`.kiro/docs/agent-loop-operation.mmd`).

3. **Session 3: Backlog #5 Completed & Merged — Test/Coverage Hardening, SAST, Coverage Re-Measurement**
   * **Period:** `2026-09-02` (continues from Session 2)
   * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-02.md`](.kiro/docs/session-summary-2026-09-02.md)
   * **Focus Areas:**
     * Committed pending Session-2 docs; fast-forward merged the full `feat/*` chain into `main` (`3804f5b`).
     * Enriched the backlog table with Rationale, Architecture-Alignment, and Effort columns.
     * **Completed & merged Backlog #5:** spec + 25 deterministic unit tests, optional non-gating Semgrep SAST, FR-4 offline E2E crash-isolation demo (Demo 12, harness relocated to `scripts/run-demos.nu`).
     * **Coverage re-measurement (first-party llvm-tools):** `src/agent_loop.rs` **46.8% → 64.7% line** — a *unit-test* methodology, not comparable to Session 2's live-harness 72.9%. Documented + reproducible via `argc test-coverage`.
     * Circuit-breaker/cost logic extracted for testability (behavior-preserving). Added **Backlog #11** (mock-Client test seam, Low priority).
     * **State:** all local, `main` 136 commits ahead of `origin/main`, nothing pushed (intentional). Open: `$0.000000` cost-estimator bug (unlogged).

4. **Session 4: Backlog #6 Designed (umbrella #6a–#6d) & #6a Implemented — Tool Safety Modes & Actuation Governance**
   * **Period:** `2026-09-02` (distinct session, same day as Session 3)
   * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-02-session4.md`](.kiro/docs/session-summary-2026-09-02-session4.md)
   * **Focus Areas:**
     * Extended design conversation turned Backlog #6 from a ~150-250-line binary mask into a **four-part umbrella (#6a–#6d)**: a layered decision funnel (non-pardonable Protected Policy File → 5-tier blast radius + orthogonal *proven* reversibility → stricter-only `%assess-risk%` LLM evaluator → HMAC-authenticated file-based escalation/control with human-in-the-loop). Graceful degradation: `#6d → #6c/#6b block → #6a mask`.
     * Wrote the umbrella spec [`.kiro/specs/tool-safety-modes/`](.kiro/specs/tool-safety-modes/) (requirements/design/tasks) and folded #6a–#6d into `backlog.md`/`progress.md` (commit `c28dfd5`).
     * **Implemented Backlog #6a (deterministic capability mask):** `ToolMode`/`SafetyClass` on `FunctionDeclaration`, `AICHAT_CAPABILITY_MASK=readonly` propagated to sub-agents, `capability_denied` gate in `eval_single_tool`; unclassified/MCP tools reserved to humans. +8 tests, suite 352→360, 0 fail, clippy clean (commit `8994922`).
     * **State:** on branch `feat/tool-safety-6a` (2 ahead of `main`); `main` 140 ahead of `origin/main`, nothing pushed (intentional). **Next: #6b** off `feat/tool-safety-6a`.

5. **Session 5: Backlog #6b Implemented & Hardened — Tiers, Reversibility, Policy & Authority Ceiling; Tool Classification; Doc Consolidation**
   * **Period:** `2026-09-02` (distinct session, same day as Sessions 3 & 4)
   * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-02-session5.md`](.kiro/docs/session-summary-2026-09-02-session5.md)
   * **Focus Areas:**
     * **Implemented Backlog #6b:** new `src/safety.rs` (5-tier `BlastRadius`, orthogonal *proven* reversibility via `required_authority`, `AuthorityCeiling`, `PolicyFile` YAML loader), top-level `safety:` config, `AICHAT_AUTHORITY_CEILING` propagation, over-ceiling → `authority_exceeded` / policy → `policy_forbidden` (commit `7de3291`).
     * **Hardened #6b** with four settled decisions: **Decision B** (delegation is orchestration, NOT gated), **Catastrophic hard-floor clamp** (proven reversibility never discounts Catastrophic below human), **`ToolBlocked` trace event**, and `AICHAT_SAFETY_POLICY_FILE`/`AICHAT_SAFETY_DEFAULT_CEILING` env overrides (commit `a3e8eb6`). Suite **386 unit / 394 workspace, 0 fail**.
     * **Classified all 31 llm-functions tools** (`# @meta risk`) and extended `build-declarations.{sh,js,py}` to emit `risk`/`reversible` — companion repo, own branch `feat/tool-safety-classification` (`2989f26`). Principle: *classify tools, don't loosen the engine.*
     * **Demos 13–15** (policy forbid / authority ceiling / arg-sensitive escalation) added; whole harness standardized on `gemini-2.5-flash`. 3 residual soft-fails (3/6/9) are model-phrasing/tmux artifacts, not bugs.
     * **Consolidated** `roadmap.md` + `progress.md` + `backlog.md` into a single `roadmap.md` (Roadmap / Traction+canonical Status Table / Backlog views, anti-drift rule); deleted the two merged files; repointed live nav links.
     * **State:** on branch `feat/tool-safety-6b` @ `a3e8eb6` (unmerged); documentation batch uncommitted; nothing pushed (intentional). **Next: #6c** (`%assess-risk%` LLM evaluator) off `feat/tool-safety-6b`.

6. **Session 6: Backlog #6c Committed & #6d Part 1 — mTLS Escalation Channel Transport + Auth Core**
   * **Period:** `2026-09-02` (distinct session, same day as Sessions 3–5)
   * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-02-session6.md`](.kiro/docs/session-summary-2026-09-02-session6.md) · **Continuation guide:** [`.kiro/docs/handover-6d-part2.md`](.kiro/docs/handover-6d-part2.md)
   * **Focus Areas:**
     * **Committed Backlog #6c** (`58af926`) — the `%assess-risk%` LLM evaluator overlay (stricter-only, `Safe` fast-path, fail-toward, raise-only `RiskCache`).
     * **Evaluated the #6d transport/dependency question** with the user → **Path 1′**: build mutual-TLS now over the in-tree `tokio-rustls` with **hand-rolled length-delimited JSON framing — NOT WebSocket** (deferred behind the `EscalationTransport` trait). Dependency audit: fork had added only 1 crate since upstream; declined `tokio-tungstenite`. Only new crate `rcgen` (+ tiny `yasna`); `rustls` stays single-version, no OpenSSL.
     * **Implemented + committed Backlog #6d part 1** (`d40bccc`) — new `src/escalation.rs`: mTLS transport, ephemeral `rcgen` per-tree cert, fingerprint-pinning rustls verifier (**fail-closed**), **channel-bound HMAC** child auth (no static token), typed `Upstream`/`Downstream` protocol + length-delimited framing. +14 tests incl. **end-to-end localhost mTLS handshake** (legit authenticates; wrong fingerprint rejected by child; wrong tree-secret rejected by parent). Suite **424 unit / 432 workspace, 0 fail**.
     * **Wrote + committed a #6d part-2 handover doc** (`208f31c`) for the loop-integration half.
     * **State:** on branch `feat/tool-safety-6d` @ `208f31c` (unmerged); nothing pushed (intentional).

7. **Session 7: Backlog #6d Part 2 Completed & Committed — Persistent Per-Process mTLS Connection & Escalation Integration**
   * **Period:** `2026-09-04`
   * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-04-session7.md`](.kiro/docs/session-summary-2026-09-04-session7.md)
   * **Focus Areas:**
     * **Completed & committed Backlog #6d Part 2** (`f57a7e6`), completing the entire Tool Safety Modes & Actuation Governance umbrella (#6a–#6d).
     * **Persistent Per-Process mTLS Connection:** Transitioned inter-agent transport to a single persistent per-process mTLS connection (`ChildEscalationClient`) multiplexing `Hello` → `Events` → `Escalations` ↔ `Verdicts` → `Results`/`Errors`.
     * **Actor Serialization & Demux:** Dedicated background single-writer actor exclusively owns `WriteHalf`, preventing frame interleaving; demuxed reader task owns `ReadHalf` and resolves in-flight escalations via an in-memory `oneshot` registry keyed by `escalation_id`.
     * **Event Backpressure & Fail-Closed:** Non-blocking 1024-bounded MPSC with drop-newest on saturation for events; immediate fail-closed cancellation of all pending oneshots on socket EOF/drop without waiting for timeouts; terminal result delivery with flush-ack before process exit.
     * **Loop Integration & HITL:** Connected `eval_single_tool` to persistent escalation client, upward parent propagation, interactive single-key HITL CLI prompt (`[c]ontinue | [h]alt | [r]evert | [e]xplain | [g]uide`), headless Layer-3 fail-closed mode, and parent trace rendering (`[child <agent_id>] ...`).
     * **Durable Rollback Journal:** Append-only on-disk `RollbackJournal` under `$XDG_RUNTIME_DIR/aichat/journals/` with strict `0600` permissions and atomic replay.
     * **Verification:** Suite **446 pass, 0 fail** (438 unit + 5 catalog + 3 integration, +20 tests from #6d), `cargo clippy --all-targets -- -D warnings` with 0 warnings, **16/16 demos pass** (`scripts/run-demos.nu`, with Demo 16 verifying genuine subprocess offline fail-closed and 0600 journal durability).
     * **State:** on branch `feat/tool-safety-6d` @ `f57a7e6`; local-only (nothing pushed).

8. **Session 8: %assess-risk% Code-Aware Safety Evaluator & Complete Parameter Classification**
   * **Period:** `2026-09-07`
   * **Focus Areas:**
     * **Code-Aware Evaluator Overlay (#6c Enhancement):** Eliminated black-box tool evaluation where the `%assess-risk%` LLM only saw tool names and arguments without knowing what the tool actually does.
     * **Dynamic Tool Resolution:** Added `resolve_tool_implementation()` in `src/agent_loop.rs` to dynamically resolve local tool scripts (`tools/*.{sh,py,js,...}`), follow `bin/` symlinks, detect binary vs UTF-8 files, and enforce a 4KB text budget.
     * **Semantic Header Extraction:** Added `parse_script_header_comments()` and `extract_declaration_context()` in `src/safety.rs` to extract `@describe` functional docstrings, multi-line comment notes, `@option` parameter definitions, and `@env` variables.
     * **Enriched Context Schema:** Updated `build_evaluator_context()` in `src/safety.rs` to pass `declaration`, `implementation`, `invocation` preview, and `arguments`.
     * **Evaluator Prompt Alignment:** Updated `assets/roles/%assess-risk%.md` to instruct the evaluator to contrast declared `@describe` intent against the actual script mechanics, trace argument flow (e.g. `eval`, `rm`), detect hardcoded side effects, and verify guards (`guard_operation.sh`).
     * **Live Verification & Tracing:** Added `[%assess-risk% evaluator response]` trace logging when `show_trace` is enabled. Verified with live `execute_command` call with Gemini 2.5 Flash, confirming the model accurately analyzes the `eval` execution mechanics. Committed in `aichat` (`4930c98`).
     * **100% Parameter Classification in `llm-functions`:** Annotated and exported `mode`, `risk`, and reversibility across all 31 root tools and all agent subcommands (`coder`, `demo`, `json-viewer`, `orchestrator`, `researcher`, `sql`, `todo`). Committed in `llm-functions` branch `feat/tool-safety-classification` (`83dea2e`).
     * **Specification & Documentation Alignment:** Updated `.kiro/specs/tool-safety-modes/` (`requirements.md`, `design.md`, `tasks.md`), `.kiro/docs/roadmap.md` (canonical status table, 6c narrative, decisions log, backlog), and `.kiro/architecture.md`.
     * **Verification:** Suite **452 pass, 0 fail** (444 unit + 5 catalog + 3 integration); `argc test` 100% passing in `llm-functions`; **all 16 demos pass** in `scripts/run-demos.nu`.
  9. **Session 9: Option B Pre-flight Opportunistic Remediation, Clamp Reversibility Fix & Live Safety Demos 17–20**
   * **Period:** `2026-09-07`
   * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-07-session9.md`](.kiro/docs/session-summary-2026-09-07-session9.md)
   * **Focus Areas:**
     * **Option B Pre-flight Remediation:** Solved the chicken-and-egg gating paradox where tools declaring `reversible-via backup` (e.g. `fs_write`) were blocked by authority ceilings before reaching the point where backups are made. The engine upfront creates an atomic backup in the durable rollback journal, stepping down required authority (`one_step_down(Disruptive) = Reversible`) and permitting autonomous actuation.
     * **Monotone Clamp Reversibility Bug Fix:** Fixed `clamp_verdict` to step down the evaluator's raw risk assessment when `reversible == true` before computing `stricter_of`, preserving the reversibility discount when evaluator agrees with tool tier.
     * **Evaluator Rollback Awareness:** Added `"rollback_mechanism": "atomic pre-mutation backup in durable rollback journal"` to evaluator context and updated `%assess-risk%.md` prompt.
     * **Role Front-Matter Model Support:** Enabled direct `model:` declaration in `%assess-risk%.md` front-matter, honoring dedicated models without mandatory `config.yaml` edits.
     * **Structured Safety Trace Events:** Added live trace rendering for gate passage, preflight reversibility application, evaluator responses, and block reasons.
     * **Live Demos 17–20:** Added Demo 17 (Happy Path), Demo 18 (Option B Pre-flight Remediation), Demo 19 (Authority Ceiling Fail-Closed), and Demo 20 (Orchestrator to Coder Multi-Process Escalation).
     * **Verification:** Suite **461 pass, 0 fail** (453 unit + 5 catalog + 3 integration); `cargo clippy --all-targets -- -D warnings` clean; all 20 demos passing in `scripts/run-demos.nu`.
     * **State:** on branch `feat/tool-safety-6d` @ `2a7da73`; local-only (nothing pushed).

  10. **Session 10: Supervisory Governance & Risk Assessment in Multi-Agent Escalation ("The Should Gate")**
    * **Period:** `2026-09-08`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-08-session10.md`](.kiro/docs/session-summary-2026-09-08-session10.md)
    * **Focus Areas:**
      * **The Should Gate:** Closed the critical gap where supervisors approved escalations simply because their ceiling allowed it ("can != should").
      * **Parent Policy Enforcement:** Supervisor enforces its own `PolicyFile` against child tool calls and args; explicit `Forbid` immediately halts (`policy_forbidden`).
      * **Anti-Spoofed Static Tier Floor:** Enforces `max(supervisor_declared_tier, esc.blast_radius)` and validates tool reversibility declarations before honoring reversibility claims.
      * **Supervisory %assess-risk% Invocation:** Calls evaluator with extended supervisory context (child ID, depth, stated reason, enrichment, declaration metadata, 4KB script source code, invocation command line).
      * **Strict Clamping & Routing:** Pure `supervisory_verdict_decision` helper clamps strictly; `Low` confidence fails toward `Human`; within-ceiling approvals attach evaluator rationale in `added_context`; over-ceiling escalates upward or prompts human operator.
      * **Live Verification:** Verified end-to-end with live Demo 20 under Gemini 2.5 Flash showing supervisor risk assessment and approval trace; test suite **467 pass, 0 fail** (459 unit + 5 catalog + 3 integration).
      * **State:** on branch `feat/tool-safety-6d`; local-only (nothing pushed).

  11. **Session 11: Permission vs. Authorization Boundary & Bounded Re-Delegation (FR-6d.18–FR-6d.21)**
    * **Period:** `2026-09-08`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-08-session11.md`](.kiro/docs/session-summary-2026-09-08-session11.md)
    * **Focus Areas:**
      * **Hard Process Capability Boundary:** Established strict engine separation between Permissions (technical process sandboxing) and Authorization (supervisory goal alignment). Removed mTLS escalation for `capability_denied` so `readonly` processes cannot have their capability mask elevated in-flight.
      * **Hierarchical Upfront Provisioning (`DelegatedPermissions`):** Orchestrator provisions sub-agents with execution capabilities at delegation time via typed schema (`permissions: { mask, ceiling }`). Engine validates `requested <= parent` and clamps closed on malformed inputs.
      * **Sub-Agent Unwind & Clean Exit:** Sub-agents tripping `capability_denied` immediately halt, unwind journal entries (`journal.replay_last()`), emit `AgentLoopEvent::CapabilityBlocked`, and return structured JSON (`status: "permission_blocked"`).
      * **Bounded Re-Delegation Circuit Breaker:** Orchestrator ingests `permission_blocked` results, evaluates whether to re-delegate with mutating permissions or consult the user, capped by a per-`(agent, task)` circuit breaker (2 attempts).
      * **Live Verification:** Verified via live Demo 21 under Gemini 2.5 Flash; test suite **468 pass, 0 fail**; companion `llm-functions` schema committed.
      * **State:** on branch `feat/tool-safety-permission-boundary`; local-only.

  12. **Session 12: Interactive Debug Stepping, Observability Dialog Trace, Visual Hierarchy & FIFO Event Pipeline**
    * **Period:** `2026-09-08`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-08-session12.md`](.kiro/docs/session-summary-2026-09-08-session12.md)
    * **Focus Areas:**
      * **Interactive Debug Stepping (`--debug` / `-d`):** Added interactive step-pause to `scripts/run-demos.nu`, enabling users to inspect tests one by one or abort with `q`.
      * **Selective Demo Execution (`--demo (-t) <ID>`):** Filter and execute single targeted demos (e.g. `./run-demos.nu --demo 1` or `-t 10b`) with input validation against all 22 test suites.
      * **Natural Language Demo Descriptions:** Formatted objective headers (`ℹ <description>`) across all 22 demos in `scripts/run-demos.nu`.
      * **Visual Hierarchy Indentation & Colors:** Root agent (`orchestrator`) renders left-most; subagents (`coder`, `researcher`, etc.) indent 4 spaces per nesting depth; unique ANSI colors assigned to agents.
      * **LLM Dialog Observability (`--dialog`) with Payload Truncation:** Complete prompt and response tracing with full `[system]` preservation and top/bottom 20-line payload truncation.
      * **Unified Event Pipeline (Option 1):** Added `AgentLoopEvent::DialogBlock` to route dialog blocks through the existing MPSC event queue, eliminating chronological race conditions with tool completions and guaranteeing 100% causal FIFO order across all 4 observability tiers.
      * **Live Verification:** Unit suite **469 pass, 0 fail**; verified with live Demos 1, 3, 5, 10b; debug and release builds fully up to date.
      * **State:** on branch `feat/tool-safety-permission-boundary`; local-only.

  13. **Session 13: Safety Evaluation Architecture Audit, RiskCache Data Model & Execution Ground Truth**
    * **Period:** `2026-09-08`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-08-session13.md`](.kiro/docs/session-summary-2026-09-08-session13.md)
    * **Focus Areas:**
      * **RiskCache Data Model Deep-Dive:** Formalized exact specification of `RiskCache`: an in-memory `HashMap<String, RequiredAuthority>` acting as a raise-only authority floor (not an execution permit or raw LLM output).
      * **Architectural Tradeoff: Caching `RiskVerdict` vs. Scalar Floor (FR-6c.9):** Proved that caching the structured `RiskVerdict` (`tier`, `confidence`, `rationale`, `concerns`) is strictly superior to caching the scalar floor. Retains evaluator rationale on cache hits (fixing `rationale: None`), maintains dynamic reversibility at act-time, and preserves monotonic safety invariants via act-time `clamp_verdict`.
      * **Demo 3 Forensic Trace Analysis & Double-Evaluation Elimination (FR-6d.22):** Dissected the Demo 3 execution where `coder` and `orchestrator` executed two near-identical consecutive `%assess-risk%` prompts. Proved that the net difference is superficial supervisory metadata (`intent` and `supervisory_request`), and designed downward propagation of `RiskVerdict` and `ExecutionPermit` in `VerdictMsg` to eliminate redundant second-round evaluations.
      * **Actuation Ground Truth vs. Declarative Metadata Noise (FR-6c.10):** Addressed the security anti-pattern of flooding the risk assessor with OpenAPI parameter schemas (`permissions_mask`, `permissions_ceiling`, etc.) while reporting `implementation: {"type": "unknown"}`. Designed multi-tool script extraction for `tools.sh` so the assessor audits the actual bash function code and target paths directly.
      * **Roadmap & Spec Crosswalk:** Added FR-6c.9, FR-6c.10, FR-6d.22 to `requirements.md` and Tasks 6c.13, 6c.14, 6d.27, 6d.28 to `tasks.md`.
      * **State:** Working tree clean (`aichat` and `llm-functions` both clean on `feat/tool-safety-permission-boundary`), local-only.

  14. **Session 14: Verdict-Level Caching, Execution-Level Ground Truth Inspection & Downward Supervisory Propagation**
    * **Period:** `2026-09-08`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-08-session14.md`](.kiro/docs/session-summary-2026-09-08-session14.md)
    * **Focus Areas:**
      * **Verdict-Level Caching (`FR-6c.9` / Task `6c.13`):** Caches full structured `RiskVerdict` (tier, reversible, confidence, rationale, concerns) in `RiskCache` instead of scalar floor. Preserves evaluator rationale for observability previews, enables dynamic act-time reversibility discounting via `clamp_verdict`, and maintains non-pardonable monotonic floors.
      * **Execution-Level Ground Truth Inspection (`FR-6c.10` / Task `6c.14`):** Replaced `"type": "unknown"` fallback for multi-tool scripts (e.g. `agents/<agent>/tools.sh`) by adding `extract_shell_function` and `resolve_tool_implementation` agent routing. Assessor inspects the exact target bash function code and doc comments, stripping out OpenAPI schema noise (`permissions_*`, `__*`).
      * **Downward Supervisory Verdict & Permit Propagation (`FR-6d.22` / Task `6d.27` & `6d.28`):** Supervisor passes `token` (permit) and `risk_verdict: Option<RiskVerdict>` in `VerdictMsg`. Child seeds `RiskCache` on `Continue` verdict and satisfies Gate 3 autonomously without duplicate risk evaluations or second-round escalations.
      * **Verification:** Suite **481 pass, 0 fail** (473 unit + 5 catalog + 3 integration); `cargo clippy --all-targets -- -D warnings` clean; live Demo 3 verified green with ground-truth bash inspection and zero redundant evaluations.
      * **State:** on branch `feat/tool-safety-permission-boundary`; local-only.

  15. **Session 15: Unbiased Grounded Risk Assessment & Full Untruncated Trace Observability**
    * **Period:** `2026-09-08` $\rightarrow$ `2026-09-09`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-08-session15.md`](.kiro/docs/session-summary-2026-09-08-session15.md)
    * **Focus Areas:**
      * **Unbiased, Grounded Risk Assessment (`FR-6c.11` / Task `6c.15`):** Removed anchoring bias, pre-classified outcome hints (`static_tier`, `declaration.safety`, `# @meta risk` leaks in extracted source), and prompt directives ("you may only make an action STRICTER than static_tier"). Stripped dead OpenAPI parameter schemas (`declaration.parameters`). Evaluator receives 100% concrete execution ground truth: tool name, resolved CLI invocation string, runtime argument values, operational intent, flattened script source code (with metadata tags stripped), and active rollback safeguards (`rollback_mechanism` if proven reversible).
      * **The LLM Ranks; Rust Enforces:** The LLM acts as an unconstrained, independent auditor that ranks blast radius (`safe` $\rightarrow$ `catastrophic`) and confidence (`low` $\rightarrow$ `high`); the Rust engine mathematically enforces the non-pardonable catalog and policy base floor via `clamp_verdict`.
      * **Full Untruncated Trace Observability (`FR-6d.23` / Task `6d.29`):** Added engine-level configuration `dialog_no_truncate` (CLI flag `--dialog-no-truncate`, env var `AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE`) to bypass payload truncation in `format_messages_dialog`, `format_llm_response`, and `truncate_payload_dialog`. Added `--no-truncate` (`-n`) flag to `scripts/run-demos.nu`, bypassing both aichat dialog payload truncation and demo runner output capping.
      * **Verification & Testing (`Task 6d.30`):** Suite **485 pass, 0 fail** (477 unit + 5 catalog + 3 integration); `cargo clippy --all-targets -- -D warnings` clean; live Demo 3 verified green with 100% grounded facts payload and full untruncated trace.
      * **State:** on branch `feat/tool-safety-permission-boundary`; local-only.

  16. **Session 16: Prohibition of Downward Permit Propagation & Hard Child Authority Ceilings**
    * **Period:** `2026-09-09`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-08-session16.md`](.kiro/docs/session-summary-2026-09-08-session16.md)
    * **Focus Areas:**
      * **Elimination of Downward Permits (`FR-6d.24` / Task `6d.31`):** Complete elimination of downward execution permits (`token`, `supervisory_approved`). `VerdictMsg` stripped of `token` and `risk_verdict`. Supervisor cannot issue pass-through tokens to override child authority ceilings.
      * **The LLM Risk Assessment is NOT a Pardoner:** Affirmed core invariant that `%assess-risk%` is strictly an extra check to prevent dangerous actions. It can only tighten restrictions (raise tiers, require human approval or halt on concerns); it must never be used to relax permissions or policies.
      * **Hard Authority Ceiling Sandboxing (Gate 1 & Gate 2 Unification):** Sub-agents cannot escalate over mTLS to elevate authority ceilings in-flight. When an action exceeds the sub-agent's ceiling (`required > child_ceiling`), actuation is blocked immediately (`authority_exceeded`). The child unwinds pre-mutation journal entries, halts, and returns structured `status: "permission_blocked", reason: "authority_exceeded"` to the parent orchestrator.
      * **Bounded Orchestrator Re-Delegation:** The parent orchestrator ingests the structured block and re-delegates with the required ceiling upfront (subject to the 2-attempt circuit breaker) or acts directly.
      * **Verification & Testing (`Task 6d.32`):** Suite **486 pass, 0 fail** (478 unit + 5 catalog + 3 web asset security); `cargo clippy --all-targets -- -D warnings` clean; `cargo build --release` clean; live Demo 16, Demo 20, and Demo 3 verified 100% green with zero downward permits and clean re-delegation. Committed in `de0f501` and `1292218`.
  17. **Session 17: Hierarchical Guide Rails, Semantic Role Badging & Turn Delta Observability**
    * **Period:** `2026-09-09`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-09-session17.md`](.kiro/docs/session-summary-2026-09-09-session17.md)
    * **Focus Areas:**
      * **Hierarchical Guide Rails (`FR-6d.25` / Task `6d.33`):** Draw colored vertical guide rails matching the agent palette across all lines of dialog output to anchor recursion depth visually.
      * **Asymmetric Framing:** Differentiate prompts (`📥 PROMPT`) from generations (`📤 RESPONSE`) with directional headers and frames.
      * **Semantic Role Badging & Boilerplate Dimming:** Style role headers semantically (`[user]`, `[assistant]`, `tool_calls:`, `tool_result:`) and dim static instructions.
      * **Multi-Turn System Prompt Folding:** Collapse static unchanged system prompts on `turn > 1` (`[system: <N> lines instructions unchanged]`).
      * **Turn Delta Highlighting:** Highlight `⚡ [new]` inputs while dimming `[history]`.
      * **ANSI-Aware Soft-Wrapping & Guide Rail Continuity (Task `6d.35`):** Dynamically soft-wrap long lines at word boundaries without breaking ANSI codes, ensuring all continuation lines prepend vertical guide rails with context-appropriate hanging indents; responsively clamp box borders to terminal width.
      * **Verification & Testing (Tasks `6d.34` & `6d.36`):** Suite **494 pass, 0 fail** (486 unit/integration + 5 catalog + 3 web asset security); `cargo clippy --all-targets -- -D warnings` clean; `cargo build --release` clean; live Demos 3, 4, and 5 verified 100% green with visual guide rails, soft-wrapping, folded prompts, and zero repetition illusion.
      * **State:** on branch `feat/tool-safety-permission-boundary`; local-only.

  18. **Session 18: Unified ALLOW / BLOCK Governance Nomenclature, Explicit Blocked Comparisons & Debug Journal Inspection**
    * **Period:** `2026-09-09`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-09-session18.md`](.kiro/docs/session-summary-2026-09-09-session18.md)
    * **Focus Areas:**
      * **Consolidated Scannable Grammar (`FR-6d.27` / Task `6d.38`):** Trace events across pass and block follow `<VERB> <tool>: <lhs> <op> <rhs>`, with `risk` always on LHS and `ceiling` always on RHS (`ALLOW <tool>: risk <tier> <= ceiling <tier>` vs `BLOCK <tool>: risk <tier> > ceiling <tier>`), plus capability mask blocks (`BLOCK <tool>: read-only mask (mutating tool; unwound: true)`).
      * **Single-Source Risk Token Derivation:** Implemented pure helper `format_risk_token(static_tier, required, mechanism)` formatting effective parenthetical qualifiers (`(effective, <why>)`), derived upfront from authoritative state to eliminate dual-path drift.
      * **Human Approval Prompt Banner:** Updated `prompt_human_verdict` to display non-colliding header `[HUMAN APPROVAL REQUIRED] <tool>` and threaded `ceiling: AuthorityCeiling` through all call sites.
      * **Debug Rollback Journal Inspection (`FR-6d.26` / Task `6d.37`):** Added `--debug` flag (and `AICHAT_AGENT_LOOP_DEBUG`) to display full rollback journal entry metadata (`target_path`, `artifact_path`, `undo_command`, compact `args`) formatted under guide rails, strictly omitting backup contents.
      * **Deterministic Agent Petnames:** Introduced disposable petnames derived via dual 32-bit integer mixes (`format_agent_pid(pid)` $\to$ `12345 (AstuteRobin)`).
      * **Bounded Helper Script Resolution:** Resolved referenced helper scripts (`utils/guard_path.sh`) safely within a 4KB budget and directory containment.
      * **Verification & Testing (Task `6d.39`):** Suite **498 pass, 0 fail** (490 unit/integration + 5 catalog + 3 web asset security); `cargo clippy --all-targets -- -D warnings` clean; `cargo build --release` clean; live Demo 16 verified green; all demos in `scripts/run-demos.nu` tolerant of new grammar.
      * **State:** on branch `feat/tool-safety-permission-boundary`; local-only.

  19. **Session 19: Link-Only Web Search (`--links`), Sub-Agent Multi-Step Research Pipeline & Empty-Response Resilience**
    * **Period:** `2026-09-09`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-09-session19.md`](.kiro/docs/session-summary-2026-09-09-session19.md)
    * **Focus Areas:**
      * **Link-Only Grounded Search (`--links`):** Added `# @flag --links` to `web_search_aichat.sh` instructing Gemini grounding to return strictly formatted `[Page Title](URL) - 1-sentence summary` entries rather than synthesizing an essay. Regenerated tool declarations in `agents/researcher/functions.json`. Verified followable Google Vertex AI Search grounding redirect URLs via `curl -fsSL` and `html-to-markdown`.
      * **Multi-Step Research Pipeline:** Updated `researcher` agent instructions (`agents/researcher/index.yaml`) to discover links via `web_search` with `links=true` on turn 1, then fetch 2–4 pages with `fetch_url_via_curl` concurrently, completely breaking the 3-tier echoing cycle.
      * **Empty LLM Response Detection & Automatic Retry:** Diagnosed root cause of transient 0.9s empty responses from Gemini where 0 tokens caused `agent_loop.rs` to treat empty outputs as premature `LoopComplete`. Added automatic exponential backoff retry in `call_llm_raw` (`MAX_EMPTY_RETRIES = 2`) with an explicit error bail if unrecovered.
      * **Streaming Error Catching:** Caught provider `blockReason` and non-`STOP` finish reasons (such as `RECITATION`) in `gemini_chat_events` streaming handler.
      * **Demo Runner Stepping Order:** Ensured demo header, natural-language description, and command line render before the interactive pause in all 22 demos in `scripts/run-demos.nu`.
      * **Verification:** Suite **498 pass, 0 fail** (490 unit/integration + 5 catalog + 3 web asset security); `cargo build --release` clean; live Demo 4 executed end-to-end with 4-turn multi-step research execution.
      * **State:** on branch `feat/tool-safety-permission-boundary`; local-only.

  20. **Session 20: Agent Loop History Preservation, Sub-Agent Turn Budget Loop Fix & URL Fetch Resilience**
    * **Period:** `2026-09-09`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-09-session20.md`](.kiro/docs/session-summary-2026-09-09-session20.md)
    * **Focus Areas:**
      * **Diagnosed Demo 5 Turn-Budget Exhaustion Loop:** Uncovered multi-layer bug where `fetch_url_via_curl` was executed repeatedly without advancing history until exhausting the 20-turn limit.
      * **Eliminated `is_all_done` Tool Result Drop (`src/agent_loop.rs`):** In `eval_tool_calls_parallel`, removed the legacy check that dropped tool results and returned `Ok(vec![])` when all results were `"DONE"`. In multi-turn agent loops, dropping tool results erases execution history, causing the LLM to receive identical prompts and re-request tools indefinitely.
      * **Enriched Tool Execution Error Reporting:** Included underlying error details `{e}` in `tool_execution_error` messages passed back to the model.
      * **Hardened Shell Tool Pipeline (`fetch_url_via_curl.sh`):** Added `set -eo pipefail` so curl failures (such as 404s) are not silently masked by `html-to-markdown`. Added modern browser User-Agent header and `-m 30` transfer timeout.
      * **Canonical URL Grounding & Non-Interactive CLI Invocation (`web_search_aichat.sh` & `summarize_text.sh`):** Instructed Google grounding to return direct canonical URLs rather than transient search redirect tokens. Added `-S` (`--no-stream`) to avoid terminal cursor read timeouts in subshells.
      * **Selective Demo Runner Pausing (`scripts/run-demos.nu`):** Aligned `should_pause` with `--debug` so automated/scripted single-demo runs (`--demo <N>`) execute non-interactively.
      * **Verification & Testing:** Added unit test `test_eval_tool_calls_parallel_preserves_all_results_without_dropping`; full suite **499 pass, 0 fail** (491 unit/integration + 5 catalog + 3 web asset security); release binary rebuilt; live Demo 5 passed with parallel researchers completing in 5 turns and synthesizing both topics cleanly.
      * **State:** on branch `feat/tool-safety-permission-boundary`; local-only.

  21. **Session 21: Nanoworker Traceability, British Humour Petnames, Ephemeral Agent Colors & Global Leftmost Timestamps**
    * **Period:** `2026-09-10`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-10-session21.md`](.kiro/docs/session-summary-2026-09-10-session21.md)
    * **Focus Areas:**
      * **Nanoworker Traceability (`@meta nano true`) & Labeling (`nano-<tool_name>`):** Annotated ephemeral utility tools (`web_search_aichat.sh`, `summarize_text.sh`) with `@meta nano true`. Labeled nanoworkers explicitly as `nano-<tool_name>` (e.g., `Agent nano-web_search`, `Agent nano-summarize_text`) while maintaining the invoking parent agent in `AICHAT_INVOKING_AGENT` to preserve color and petname inheritance (`nano-<ParentPetname>-<Seq>`).
      * **Compact British Humour Petnames:** Shortened adjectives and nouns to two 32-element arrays of witty British humor words (<= 9 chars), reducing average petname length from 16.5 to 10.8 chars.
      * **Ancestor Visual Rails & Indentation:** Widened ancestor vertical guide rails to 6 columns (`│     `) and indented trace event lines by 4 spaces (`    [... starting]`), preventing clumping and anchoring visual hierarchy.
      * **Ephemeral 11-Color Agent Palette:** Deterministic 11-color soft ANSI palette derived via `djb2_hash(petname)` and inherited by child processes via `AICHAT_AGENT_COLOR` for stable visual identity.
      * **Contrasting Error & Escalation Styling:** Soft coral red `ERROR_COLOR` (`#e06c75` / ANSI 203) for errors and policy/authority rejections; warm amber `ESCALATION_COLOR` (`#d19a66` / ANSI 179) for escalations, human intervention, and re-delegation.
      * **Dialog Role Keyword Coloring:** Semantically styled `[user]` (Cyan), `[assistant]` (Yellow), `[system]` (Light Cyan), `[history: <role>]` (Warm Amber), and `[tool]` / `tool_calls:` (Magenta) within dialog frames.
      * **Historic Corpus Dimming & Response Blockquotes:** Rendered prior conversation history in Dark Gray (`#666666`), highlighted active turn inputs with `⚡ [new: ...]`, and dimmed Markdown blockquotes (`> ...`) in LLM responses while preserving internal code blocks.
      * **Tool Pipeline Rationalization (`fetch_and_summarize` vs `fetch_url_via_curl`):** Rationalized tool redundancy between `fetch_and_summarize` and `fetch_url_via_curl | summarize_text`. `fetch_url_via_curl` was restored to a pure URL-to-Markdown fetcher returning full Markdown directly to caller context. `fetch_and_summarize` was upgraded to native `html-to-markdown` and declared with pipe output routing to `summarize_text`. Updated `researcher` agent to use `web_search` and `fetch_and_summarize`.
      * **Non-Interactive Stream Resilience (`IS_STDIN_TERMINAL`):** Added `IS_STDIN_TERMINAL` check in `render_stream` (`src/render/mod.rs`) to prevent `markdown_stream` from attempting interactive `cursor::position()` ANSI handshakes when stdin is not a terminal (e.g. piped or in subshells). Hardened `cursor::position()` error fallback to default to `(0, 0)` rather than aborting stream execution.
      * **Verification & Testing:** Full workspace test suite **511 pass, 0 fail** (503 unit/integration + 5 catalog override + 3 web asset security); clippy clean (`-D warnings`); release binary compiled; live Demo 5 verified with parallel researchers and nanoworkers completing synthesis; live Demo 8 verified with `fetch_and_summarize` pipe routing.
      * **State:** on branch `feat/tool-safety-permission-boundary` (aichat) & `feat/fetch-url-native-html-to-markdown` (llm-functions); local-only.

  22. **Session 22: Web-Search Grounding Control (`--wslinks`), Streamlined Trace Display, Dual-Layer `MALFORMED_FUNCTION_CALL` Recovery & Truthful Failure Reporting**
    * **Period:** `2026-09-10` $\rightarrow$ `2026-09-11`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-11-session22.md`](.kiro/docs/session-summary-2026-09-11-session22.md)
    * **Focus Areas:**
      * **Web-Search Grounding Control (`--wslinks` & `AICHAT_WSLINKS`):** Added `--wslinks` CLI flag to `aichat` and `scripts/run-demos.nu`. Injected and propagated `AICHAT_WSLINKS=true` across child processes. Dynamic interpolation of `{{__researcher_search_instructions__}}` allows fast 1-turn direct grounded web search without page scrapes by default, or multi-step link exploration and scraping when `--wslinks` is enabled. Enforced `AICHAT_WSLINKS` guard in `tools/web_search_aichat.sh`.
      * **Streamlined Trace Display:** Omitted redundant initial agent and nanoworker "loop trace" starting header line (`Agent <name> loop trace:`) to conserve vertical terminal space while preserving indentation, British humor petnames, 11-color ANSI palettes, and turn tracking (`[turn 1/20] starting`). Hardened atomic terminal line writes via `write_atomic_terminal_output` to eliminate multi-process line collisions on `/dev/tty`.
      * **Non-`STOP` finishReason Bubble-Up (`src/client/vertexai.rs`):** Bubbled non-`STOP` finish reasons when candidates lack content parts, preventing silent drops on safety filters or recitation blocks.
      * **Dual-Layer `MALFORMED_FUNCTION_CALL` Resilience & Transient Retries:** Resolved parallel sub-agent crashes where Gemini 2.5 Flash under burst load hallucinated Python function syntax (e.g. `print(default_api.web_search(...))`) triggering `MALFORMED_FUNCTION_CALL`. Added explicit prompt guidance against code/namespaces in tool calls, an AST/kwargs recovery parser (`recover_malformed_function_call`, `parse_python_kwargs`) in `src/client/vertexai.rs`, and exponential backoff retries in `call_llm_raw` for transient errors (`MALFORMED_FUNCTION_CALL`, `ResourceExhausted`, `429`, `503`).
      * **Truthful Tool Failure Reporting (`src/agent_loop.rs`):** Fixed `eval_tool_calls_parallel` to inspect `value.get("error").is_some()`, truthfully rendering child process exit failures as `FAILED` in soft coral red instead of false green `completed`.
      * **URL Token Formatting & Strict JSON Constraints (`llm-functions`):** Formatted canonical URLs on dedicated lines (`Title: ...\nURL: ...\nSummary: ...`) in `tools/web_search_aichat.sh` to prevent markdown link brackets from breaking base64 tokens. Added strict JSON validation constraints in `agents/researcher/index.yaml`.
      * **Verification & Testing:** All 505 unit tests in `aichat` passing; `cargo clippy -- -D warnings` clean; 71/71 `argc test` in `llm-functions` passing; live Demo 5 verified green in both `--wslinks` (40.6s, 0 crashes, 0 retries) and default direct grounded mode (33.9s).
      * **State:** on branch `feat/tool-safety-permission-boundary` (aichat) & `feat/fetch-url-native-html-to-markdown` (llm-functions); local-only.

  23. **Session 23: Branch-Exclusive Agent Colors & Nano Caller Inheritance**
    * **Period:** `2026-09-12`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-12-session23.md`](.kiro/docs/session-summary-2026-09-12-session23.md)
    * **Focus Areas:**
      * **Branch-Exclusive Agent Colors:** Eliminated inter-agent color collisions where parallel sub-agents or sub-agents and orchestrators shared the same color. Palette index 0 (`"cyan"`) is reserved exclusively for the root orchestrator.
      * **Hierarchical Palette Stratification (`src/agent_loop.rs`):** Added `allocate_subagent_color` function partitioning `AGENT_PALETTE` so direct subagents cycle through 10 distinct non-cyan colors, while sub-subagents (`depth > 1`) are offset into higher partitions by parent sequence.
      * **Zero-Overhead Subagent Sequence Injection:** In `eval_agent_tool_subprocess`, an atomic sequence counter (`SUBAGENT_COUNTER`) allocates colors and exports `AICHAT_AGENT_COLOR` and `AICHAT_SUBAGENT_SEQ` to children without locks or filesystem registries.
      * **Nano-Subagent Caller Inheritance (`src/function.rs`):** Verified and reinforced that `# @meta nano true` tools inherit their invoking agent's color (`AICHAT_AGENT_COLOR = current_agent_color_name(&invoking_agent)`).
      * **Root Orchestrator Initialization (`src/main.rs` & `src/agent_loop.rs`):** Guaranteed root process initializes `AICHAT_AGENT_COLOR = "cyan"` when unset.
      * **Verification & Testing:** Added unit tests `test_allocate_subagent_color_exclusivity` and `test_nano_worker_inherits_caller_env_color`; all 507 unit/integration tests passing; debug build verified.
      * **State:** on branch `feat/branch-exclusive-agent-colors`; local-only.

  24. **Session 24: Empty LLM Response Retry Nudge & Graceful Synthesis Fallback**
    * **Period:** `2026-09-12`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-12-session24.md`](.kiro/docs/session-summary-2026-09-12-session24.md)
    * **Focus Areas:**
      * **Forensic Diagnosis of Sub-Agent Empty Responses (Demo 20 & 21):** Identified turn 2 empty responses (`finishReason: "STOP"` with no parts) from Gemini 2.5 Flash following successful tool completion (`fs_create`). Repeated retries with identical input hit server-side prompt cache (`cachedContentTokenCount: 781`) and deterministically returned empty responses within 300ms until bailing with `LLM returned an empty response with no text and no tool calls`.
      * **Retry Nudge Injection (`src/config/input.rs`):** Added `Input::tool_calls_mut` and `Input::append_retry_nudge`. On empty response retry, annotates the prior tool result output (or prompt text) with `[Instruction: The previous tool completed successfully. Please confirm completion to the user or summarize the result.]`. Breaks prompt cache hash across all LLM providers and instructs model to synthesize completion.
      * **Graceful Synthesis Fallback (`src/agent_loop.rs`):** Updated `call_llm_raw` to check `has_prior_tools`. When tools have already run, exhausted retries gracefully synthesize `"Tool execution completed successfully (<tool_names>)."` and complete the turn cleanly rather than bailing with an agent failure. Preserved strict failure bailout for turn-1 empty responses without tool execution.
      * **Verification & Testing:** Added unit tests `test_append_retry_nudge_with_tool_results_string_and_object` and `test_graceful_synthesis_fallback_formatting`; all 509 unit tests passing; live Demo 20 and Demo 21 verified passing cleanly with zero empty-response retries or sub-agent failures.
      * **State:** on branch `fix/empty-response-retry-nudge-and-fallback`; local-only.

  25. **Session 25: Comprehensive Prompt & Model Observability Across All Execution Paths**
    * **Period:** `2026-09-14`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-14-session25.md`](.kiro/docs/session-summary-2026-09-14-session25.md)
    * **Focus Areas:**
      * **Centralized DialogTraceSink Architecture:** Built an unbounded, asynchronous trace sink in `src/agent_loop/dialog_trace.rs` with monotonic sequence tracking, exit drain guarantees, and destination control (`AICHAT_DIALOG_OUTPUT=stderr|tty`).
      * **Cross-Process Relay Framing (`RELAY_FRAME_PREFIX`):** Implemented length-delimited JSON relay framing over stderr for generic shell tools (`run_llm_function`) and subagents, with concurrent pipe draining to prevent OS buffer stalls.
      * **Universal Model Attribution:** Wired configured model (`@ <model>`) and wire model (`[wire: <model>]`) badges across all dialog headers for root agents, subagents, nanoworkers, and internal safety evaluators (`%assess-risk%`).
      * **Semantic History Folding & Payload Capping:** Collapsed unchanged system instructions on turns > 1 (`[system: <N> lines instructions unchanged]`), dimmed prior history, highlighted delta inputs (`⚡ [new: ...]`), and capped large outputs to top/bottom 20 lines.
      * **Universal Observability Expansion:** Extended dialog tracing to session autonaming (`%create-title%`), context compression (`%summarize-session%`), natural language shell execution (`%shell%`), and OpenAI Responses API multi-agent continuations.
      * **CLI Aliases & Demo Runner Integration:** Added `--dialog` (`--show-dialog`) and `--no-truncate` (`--dialog-no-truncate`) CLI aliases and mapped them into `scripts/run-demos.nu`.
      * **Verification & Testing:** All 516 unit/integration tests passing; full live demo suite verified green across Demos 1, 2, 3, 4, 5, and 5b.
      * **State:** on branch `feat/dialog-observability-all-prompts-and-models` @ `3a1bf79`; local-only.

  26. **Session 26: Structured Plan Execution (#15) & Progressive Skill Runbooks with Provenance-Based Taint (#17)**
    * **Period:** `2026-09-15`
    * **Handoff Document:** [`.kiro/docs/session-summary-2026-09-15-session26.md`](.kiro/docs/session-summary-2026-09-15-session26.md)
    * **Focus Areas:**
      * **Backlog #15 (Spec A: Structured `_plan` & Ahead-of-Time Pre-Pass):** Designed and implemented structured `_plan` JSON Schema, flexible dual-arm parser (supporting structured steps and legacy/degraded strings), state tracker (`PlanTracker`), and live progress events (`PlanReceived`, `PlanStepUpdated`, `PlanRiskPrepassFlagged`). Implemented `plan_risk_prepass()` ahead-of-time risk checking populating the monotonic, raise-only `RiskCache`, with clean no-op degradation when #6c's `%assess-risk%` is absent.
      * **Backlog #17 (Spec B: Progressive Skill Runbooks & Taint Tracking):** Implemented `SkillRegistry` with 3-tier discovery precedence (`Workspace` > `Global` > `Builtin`). Workspace skills are automatically marked `WorkspaceTainted`. Added YAML frontmatter parser for `SKILL.md` (no mandatory hash/signature gate). Implemented agent eligibility gating (`skills: all | false | [...]`) and strict exclusion of nano utility workers (`@meta nano true` / `nano: true` / `AICHAT_AGENT_NANO=true`). System prompts are augmented with the `### Available Skills` metadata catalog only for eligible agents.
      * **Dynamic Tool Injection & Execution:** Dynamically declared and dispatched `read_skill(name)`, returning instructions, description, path, and provenance.
      * **Active Taint Lifecycle & Step Binding:** Implemented `ActiveSkillTracker` providing plan-step bound taint tracking. Loading a workspace skill activates taint (`untrusted_runbook: true` and `active_tainted_skills`). Taint is maintained across step execution and cleared upon step completion (`complete_step`) or explicit consumption.
      * **Evaluator Integration & Prepass Taint Simulation:** Added heightened scrutiny directive 5 in `assets/roles/%assess-risk%.md` and wired taint status into `src/safety.rs`. Simulated `read_skill` step loads in `plan_risk_prepass` so downstream steps evaluate with `untrusted_runbook: true`.
      * **Verification & Testing:** All 6 unit tests in `src/skill.rs` passing; all 128 tests in `agent_loop::tests` passing; all 64 tests in `safety::tests` passing; 529 total tests passing with zero failures. Clippy clean (`-D warnings`).
      * **State:** on branch `feat/structured-plan-and-skills` (`66199b4`); local-only.

  27. **Session 27: Scoped Tools Token Optimization, Progressive Runbook POC Demos, Causal Gate Sequencing & Postmarked Insights**
    * **Period:** `2026-09-16`
    * **Handoff Document:** [`lesssons-learned.md`](lesssons-learned.md) & [`skills_poc_walkthrough.md`](skills_poc_walkthrough.md)
    * **Focus Areas:**
      * **Scoped Tools Token Optimization (~95% schema reduction):** Scoped functional roles (`-r "%functions:tool1,tool2%"`) across 14 demos in `scripts/run-demos.nu`, reducing per-turn tool schema overhead from ~6,000 to ~300 tokens.
      * **Engine Safety Classification for `read_skill`:** Registered `read_skill` as `SafetyClass::Readonly`, `BlastRadius::Safe`, and `intrinsic_reversible: true` across `src/agent_loop.rs`, resolving latent fail-closed `authority_exceeded` blocks.
      * **Implicit Catalog & `read_skill` Injection for Non-Agent Roles:** Added `role.append_prompt` (`src/config/role.rs`). In `src/config/mod.rs` (`extract_role` & `select_functions`), injected `### Available Skills` catalog and `read_skill` tool whenever eligible skills exist, enabling non-agent roles to run progressive runbooks.
      * **Builtin Skill Fixture:** Created permanent version-controlled builtin skill at `assets/builtin-skills/sys_triage/SKILL.md`.
      * **Live Proof-of-Concept Demos (Demo 22 & Demo 23):**
        * **Demo 22 (Builtin, Trusted):** Progressive disclosure runbook (`sys_triage`) reading procedure via `read_skill`, capturing timestamp, inspecting hostname, writing report to file, and outputting verified summary to terminal stdout.
        * **Demo 23 (Workspace, Untrusted):** Workspace skill discovery (`repo_patcher`), provenance taint tracking (`WorkspaceTainted`), heightened scrutiny in `%assess-risk%` (`untrusted_runbook: true` $\to$ evaluated as `disruptive`), creating patch manifest artifact inside workspace, and outputting to terminal stdout.
      * **Causal Safety Gate Sequencing (`ALLOW` After Risk Assessment):** Deferred static `ALLOW` emission in `eval_single_tool` when dynamic risk assessment is required (`will_consult_risk_evaluator`). The authorization comparison `ALLOW <tool>: risk <tier> <= ceiling <tier>` is now emitted strictly **after** `%assess-risk%` completes and verifies the action.
      * **Primary Trace Taint Visibility:** Added `untrusted_runbook: bool` to `AgentLoopEvent::RiskAssessmentStart` so taint is visible directly on live terminal traces (`/dev/tty`).
      * **Workspace Target Containment & Path Harmony:** Confined Demo 23 patch log artifacts strictly inside `$d23_ws/patch.log` and harmonized prompts, runbooks, and outputs.
      * **Forensic Trace Discrepancy Analysis & Multi-Demo Hardening:**
        * **Demo 21 Sub-Agent Authority Provisioning:** Addressed failure where orchestrator re-delegated with mutating permissions but omitted authority ceiling, defaulting to `safe` and causing secondary ceiling blocks. Updated prompts and schemas to require both `permissions_mask 'mutating'` and `permissions_ceiling 'disruptive'`, and updated `AgentLoopEvent::CapabilityBlocked` to include the specific denial `reason` (`authority ceiling exceeded` vs `read-only mask`).
        * **Demo 11 TTY Stream False Negative:** Fixed assertion in `scripts/run-demos.nu` by filtering out child IPC relay frames (`[child `) from `clean11` when verifying `/dev/tty` visual trace output.
        * **Tracing & Harness Enhancements:**
          * **Turn Start Model & Token Count Tags:** Added `model: Option<String>` and `tokens: Option<usize>` to `AgentLoopEvent::TurnStart` and `AgentLoopEvent::DialogBlock`. In `format_trace_event_styled` and `format_dialog_block_with_model`, request tokens are rendered in `DarkGray` immediately preceding `@ <model>` (e.g. `286 tok @ gemini:gemini-2.5-flash`), visible in both standard trace lines (`[<agent> <pid> (<petname>) 286 tok @ <model> [turn X/Y] starting]`) and dialog frames (`┌── 📥 [<pid> <agent> 286 tok @ <model> [turn X/Y] PROMPT SUBMITTED TO LLM]`).
          * **Unescaped Evaluator Scripts & Commands:** Implemented `pretty_format_evaluator_context` and `format_evaluator_dialog_prompt` in `src/agent_loop.rs`. In `--dialog` mode, `%assess-risk%` separates system auditor instructions from the evaluated action and unescapes tool implementations (`source`), helper scripts (`helpers`), and evaluated commands/code (`arguments.command`) into readable Markdown code blocks (````bash ... ````), while strictly preserving the raw byte-for-byte JSON payload sent to the evaluator LLM over the wire.
          * **Harness Prompt Isolation & Highlighting:** In `scripts/run-demos.nu`, updated `show-cmd` to visually isolate the trailing user prompt from the wall of environment overrides and CLI flags. Prompts are rendered on their own line in highlighted `light_cyan` with a 4-space indent and empty lines before and after.
        * **Tool Safety & Multi-Tool Architecture Alignment (`llm-functions`):**
          * **`fs_create` Reversibility Classification:** Confirmed `fs_create` in `agents/coder/tools.sh` qualifies for `# @meta reversible-via backup`. The pre-flight remediation engine (`record_pre_mutation_journal_entry` in `src/agent_loop.rs`) supports both existing file snapshots (`.bak`) and new file deletions (`rm -f '<path>'`), enabling safe step-down under a `reversible` ceiling. Annotated `fs_create` with `# @meta reversible-via backup`, rebuilt `functions.json` via `argc build@agent coder`, and committed to `llm-functions` (`232354a`).
          * **Multi-Tool Isolation Architecture:** Clarified that multi-tool scripts (`tools.sh`) must never receive file-level safety metadata. Safety attributes are strictly per-subcommand (`# @cmd`), isolated via `extract_shell_function` at runtime to preserve least-privilege security boundaries.
      * **Postmarked Knowledge Base:** Created `lesssons-learned.md` (and symlinked `lessons-learned.md`) with 17 postmarked architectural insights and operational guidance for future agents.
      * **Verification & Testing:** All 542+ unit tests passing (`cargo test --bin aichat`); clippy clean with 0 warnings (`cargo clippy --bin aichat -- -D warnings`); release binary compiled (`cargo build --release --bin aichat`); Demos 4, 18, and 23 verified live in both standard and `--dialog` modes.
      * **State:** on branch `main`; local-only.




