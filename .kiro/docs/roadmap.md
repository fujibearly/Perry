# Project Perry (Agent P) — Roadmap, Traction & Backlog
*(Hardened `aichat` Fork)*

> **Codename: Perry (Agent P)** — An unpretentious, compact SRE harness engineered to disarm catastrophic infrastructure "-Inators" on 64MB bastions through deterministic containment and supervisory escalation.


**Single source of truth** for Project Perry's direction, current state, and work queue.
Consolidates what were previously three drifting docs (`roadmap.md` + `progress.md` +
`backlog.md`) into one document with **three aligned views**, all keyed by the same
item IDs (`#N`):

1. **[View 1 — Roadmap](#view-1--roadmap-strategy)** — strategic direction (the *why* and *where*).
2. **[View 2 — Traction](#view-2--traction-progress)** — current state + the canonical status table (the *where we are now*).
3. **[View 3 — Backlog](#view-3--backlog-item-detail)** — full per-item detail (the *what each item is*).

> **Anti-drift rule.** An item's **status/priority/branch/effort is defined in exactly one
> place — the [Status Table](#status-table-canonical) in View 2.** View 1 (crosswalk) and
> View 3 (item bodies) *reference* items by ID and describe strategy/detail, but must **not**
> restate status. Update status in the table only.

**Related docs:** [`glossary.md`](glossary.md) (canonical terminology) · [`architecture.md`](../architecture.md) · [`fork-philosophy-and-architecture.md`](fork-philosophy-and-architecture.md) · [`architecture-blueprint-2026-08-28-to-2026-09-02.md`](architecture-blueprint-2026-08-28-to-2026-09-02.md) · [`sre-and-supervisory-landscape.md`](sre-and-supervisory-landscape.md).

---
---

# View 1 — Roadmap (strategy)

The strategic direction — mostly cross-cutting and Layer 3/4. A roadmap strand is only
"committed work" once it has a backlog item (`#N`). Strands without one are aspirations, not
obligations. The engine (L2) is the fixed constant; L3/L4 sit *around* it and drive it via
CLI/PTY (L3) or Remote MCP/HTTP/WSS (L4) — which is why the roadmap and backlog barely overlap.

## The 4-Layer Taxonomy (orientation)

| Layer | What | Examples | Where the work lives |
|-------|------|----------|----------------------|
| **L1 — Actuation** | Deterministic CLI tools | `llm-functions` (31 Bash/`argc` tools) | Separate repo; some items touch it (#6, #10) |
| **L2 — Execution Engine** | The reasoning loop, MCP, RAG, routing, budgets | **this `aichat` fork** | **The backlog is almost entirely here** |
| **L3 — Workspace Supervisors** | Terminal multiplexing, HITL gates, worktrees | `dot-agent-deck`, `bohay`, `AoE` | External tools (adoption, not our code) |
| **L4 — Enterprise Control Plane** | Multi-tenant auth, cloud audit, fleet coordination, shared cache | `TrueForge`, `Portkey`, **Fleet Commander / "Hive-Mind"** | **Proposed only — not built, mostly out of engine scope** |

## Roadmap ↔ Item Crosswalk

Maps each strategic strand to its tracked item. **Status lives in the [Status Table](#status-table-canonical)** — the "Item" column is the link.

| Strategic strand | Layer | Item | Notes |
|------------------|-------|------|-------|
| Native in-process MCP | L2 | **#1** | Replaced the Node.js bridge. |
| Provider-agnostic agent loop | L2 | **#3** | Parallelism, budgets, sub-agents, `_plan`. |
| Declarative stream routing | L2 | **#4** | Auto-cap, pipes, file targets. |
| Deterministic test/coverage hardening | L2 | **#5** | agent_loop.rs 46.8%→64.7% line. |
| Tool safety modes / actuation governance | L1/L2 | **#6** (umbrella #6a–#6d) | Graduated, non-pardonable deterministic floor + LLM risk overlay + escalation to humans. HITL-adjacent; complements L3 approval gates; #6d forward-compatible with remote agents. |
| Session resumption / WAL | L2 | **#7** | Durable state; survives dropouts/SIGINT. |
| Context compaction | L2 | **#8** | Fallback to the delegation-first hygiene model. |
| Ephemeral Git worktree isolation | L2 | **#9** | Engine-side complement to L3 `bohay`. |
| Staged config / dry-run ops | L1 | **#10** | SRE actuation safety; mostly `llm-functions`. |
| Mock-client test seam | L2 | **#11** | Deterministic coverage of `run()` orchestration. |
| Remote MCP transports (HTTP/WSS) | L2→L4 | **#12** | The concrete engine work that lets L4 control planes drive the engine remotely. |
| Scoped shared artifact store (engine-level cross-agent memory) | L2 | **#13** | Structured, root-PID-scoped, read-mostly. Engine-level counterpart to the L4 Hive-Mind — NOT a free-form blackboard. |
| Per-machine consolidated audit log (auditability, not just observability) | L2→L4 | **#14** | Durable append-only JSONL per agent, consolidated per-machine via correlation IDs; read by external auditors/observability platforms. Distinct *audit plane* from #6d's control + rollback planes. |
| Web-search style & branch-wide grounding control (`--wslinks`) | L1/L2 | **#16** | Branch-wide toggle between direct grounded search (default 1-turn) vs. link exploration (`--wslinks` multi-step). |
| Serious Structured `_plan` / Plan-Driven Execution | L2 | **#15** | Structured step execution, progress tracking & whole-plan risk pre-pass into raise-only RiskCache. |
| Progressive Disclosure Runbooks & Skills (`read_skill`) | L2 | **#17** | Progressive runbook disclosure, 3-tier discovery, eligibility gating, nano exclusion & step-scoped taint tracking. |
| Gemini Interactions API | L2 | **#2** | Covered via OpenRouter + client loop. |
| CLI flag parity (`--show-trace/--max-turns/--max-cost`) | L2 | *(folded into #3)* | Flags exist; no distinct open item. |
| **Adopt `dot-agent-deck`** (SRE mission control, HITL cards) | L3 | 🧭 *(external, not our code)* | #1 SRE supervisor recommendation. Engine already emits the status-file/`/dev/tty` signals it consumes. |
| **Adopt `bohay`/Luvus** (worktree multiplexer, file leases) | L3 | 🧭 *(external; overlaps #9)* | #1 code-refactoring supervisor. |
| **Agent of Empires (AoE)** (tmux fleet dashboard) | L3 | 🧭 *(external option)* | Host-dependent; lower portability. |
| **Fleet Commander / "Hive-Mind"** (cloud shared semantic cache, fleet coordination) | L4 | 💭 *(concept only — see #13 for the engine-level slice)* | Not built, not in the backlog. A cloud/fleet aspiration above the engine. |
| **TrueForge / enterprise K8s gateway** (RBAC, cloud audit) | L4 | 💭 *(external / concept)* | Also `Portkey` — LLM gateway/observability. Out of engine scope entirely. |
| | | | |
| ***Distilled from TrueForge & KubeIntellect study (Session 27)*** | | | |
| Dedicated SRE Telemetry & Multi-Agent Triage (`host_env`, `host_service`, `host_resource`, `host_net`, `host_logs`, `sre` agent) | L1/L2 | **#18** | 🟢 Done. 5-Pillar SRE suite, `host_env`, `sre` agent, and parallel orchestration. |
| Autonomy Ladder (`--autonomy <readonly\|consult\|reversible>`) | L2 | **#19** | 🔧 Enhancement of **#6** (tool safety tiers) and **#6d** (escalation). Session-level posture presets. |
| Zero-Token Pre-Flight Triage (`--preflight`) | L1/L2 | **#20** | 🆕 Entirely new. Deterministic bash check short-circuits before agent loop at $0.00 cost. |
| In-Loop Clarification Tool (`ask_user`) | L2 | **#21** | 🔧 Enhancement of **#6d** (escalation). Extends the `/dev/tty`/mTLS channel from safety-only to clarification. |
| Deferred Tool Loading (lazy schema injection) | L2 | **#22** | 🔧 Enhancement of **#8** (context compaction) and **#17** (progressive disclosure). Applies progressive disclosure to tool schemas. |
| Flight Recorder & Session Replay (`--replay`) | L2 | **#23** | 🔧 Enhancement of **#7** (WAL) and **#14** (audit log). Adds hash-chaining and replay over their event stream. |
| Read-Only Batch Actuator Execution (`run_batch`) | L2 | **#24** | ⚡ Overlaps **#3** (parallel dispatch) and **#15** (plan execution). Read-only-gated multi-tool batch in a single turn. |
| Lightweight Incident Episode Store | L2 | **#25** | 🔧 Enhancement of **#13** (shared artifact store). Cross-session SQLite episode persistence — new subsystem, not a RAG extension. |

Legend: 🧭 external tool to adopt · 💭 concept only (no backlog item) · 🆕 entirely new · 🔧 enhancement of existing item · ⚡ overlaps existing items. Tracked items (`#N`) carry their status in the [Status Table](#status-table-canonical).

## Honest Status Notes

- **The "Hive-Mind" / 3-Tier Evolving Semantic RAG Cache is a concept, not code.** It appears only in the Session 1 summary (as a "formulated" idea) and in the SRE-landscape doc (as a *proposed* Layer-4 component). It is **not implemented and not in the backlog.** The only thing that exists today is the per-instance Layer-2 hybrid RAG (`src/rag/`, HNSW + BM25 + RRF) — retrieval, not shared cross-agent memory. Item **#13** captures the *engine-level, single-tree* slice of the idea; the cloud/fleet version remains an L4 aspiration.
- **The "six-capability" roadmap = items #5-#10, not a distributed-fleet phasing.** The Session-2 architecture blueprint (§5) crystallized **six high-leverage Layer-2 engine capabilities** — these are exactly items **#5, #6, #7, #8, #9, #10**. Earlier session-summary phrasing ("6-phase Distributed Fleet") was imprecise: there is no separate enumerated fleet-phasing plan; the "six" are the engine items, all tracked. The Fleet Commander itself remains a distinct, un-phased L4 concept.
- **L3/L4 tools are adoption recommendations, not fork deliverables.** `dot-agent-deck`, `bohay`, `AoE`, `TrueForge` are external projects. The engine's job is to emit the right contracts (status files, `/dev/tty` signals, Remote MCP via #12) so they can drive it — not to build them.

## The Containment Spectrum (from the Session-2 blueprint)

Containment in this system-level engine is not one mechanism but a spectrum across layers. Four distinct patterns, each mapped to its item:

| Pattern | Layer | Lifetime | Mechanism | Item |
|---------|-------|----------|-----------|------|
| **System staging / dry-run** | L1/L2 | per-mutation | write to `/tmp/staging/`, run validators (`nginx -t`, `kubectl diff`), atomic apply + `.bak` rollback | **#10** |
| **Micro-worktrees** | L2 | seconds–minutes | ephemeral detached `git worktree add /tmp/aichat-wt-<pid>`, automated diff → orchestrator consolidation | **#9** |
| **Macro-worktrees** | L3 | hours–days | long-lived developer branches, human review/merge (external: `bohay`) | *(L3 tool)* |
| **Parallel read-only swarms** | L2 | per-turn | collision-free concurrent diagnostics (already shipped) | *(part of #3)* |

Takeaway: #9 (micro) and #10 (staging) are two points on the same continuum; the macro end is deliberately delegated to an L3 supervisor rather than built into the engine.

## Suggested Sequencing (engine work only)

Item dependency notes (View 3) govern fine ordering. Strategically:

1. **High-priority safety/durability:** #6 (Tool Safety Modes) and #7 (WAL Resumption) — both High, both core to the SRE stress-test. **The entire #6 umbrella (#6a–#6d) is fully implemented, verified, and hardened with Option B pre-flight remediation and Demos 17–20**. Next: Backlog #7 (Session Resumption & WAL Journaling). A serious structured `_plan` (plan-driven execution + whole-plan red-light pre-pass, feeding #6c's raise-only cache) is split out as its own item (#15).
2. **#12 Remote MCP** when L4 integration becomes concrete (the bridge that makes any L4 control plane useful). #6d's mTLS channel and #12 share the same TLS/transport muscle — the remote-agent generalization of #6d rides on it (a WebSocket-over-routable-TLS transport added behind the `EscalationTransport` trait).
3. **#13 shared artifact store**, **#11 mock-client seam**, and **#14 audit log** are opportunistic — pick up when a dependent feature makes them cheap. #14 shares an append-only-JSONL writer with #7 (WAL) and is the natural home to fix the `$0.000000` cost-estimator bug.
4. **#2 Gemini Interactions** stays deferred unless OpenRouter coverage proves insufficient.

---
---

# View 2 — Traction (progress)

### Current State (2026-09-22 — Session 31)

**Branch:** `main` — **Project Perry (`/projects/perry`) & Innators (`/projects/innators`)**.
**Version:** v0.31.0-fork.9
**Tests:** 567 pass, 0 fail (559 unit/integration + 5 catalog-override + 3 web asset security) — suite passing; clippy clean (`-D warnings`); debug and release binaries verified.
**Runtime Environment:** Dedicated config root `~/.config/perry/` (with `functions -> ~/projects/innators`); global release binary at `~/.local/bin/perry`.
**Safety & Autonomy:** Engine default `--autonomy readonly` (Method 2, principle of least privilege; opt out via `--autonomy none`). Hierarchical `WEB_SEARCH_MODEL` fallback (`env -> config.yaml web_search_model -> model -> default`).
**#6d transport decision (Path 1′):** mutual-TLS over a raw loopback TCP stream + hand-rolled length-delimited JSON framing — **not** WebSocket (deferred behind the `EscalationTransport` trait). Only new crate `rcgen` (+ tiny `yasna`); `rustls`/`tokio-rustls` reused (single version, no OpenSSL). Child auth = channel-bound HMAC (no literal token).
**E2E demos (2026-09-21, live on `gemini-2.5-flash`):** 27 demos in `scripts/run-demos.nu`. Features unified scannable grammar (`ALLOW <tool>: risk <tier> <= ceiling <tier>`, `BLOCK <tool>: risk <tier> > ceiling <tier>`), 5-pillar SRE host telemetry actuators (`host_env`, `host_service`, `host_resource`, `host_net`, `host_logs`), dedicated `sre` specialist agent, parallel multi-agent triage sweeps (Demo 25, 4 turns), correlated incident RCA (Demo 26), 24h historical telemetry lookback with decoupled distillation (Demo 27), interactive debug stepping (`--debug`), selective targeted execution (`--demo <ID>`), pre-pause demo objective banners (`ℹ`) and command rendering, agent hierarchy 6-column guide rails (`│     `), color-coded agent traces with branch-exclusive stratified palettes, compact British humour petnames, nanoworker parent petname and color inheritance, LLM dialog observability (`--dialog`) with semantic role coloring and historic corpus dimming, untruncated trace observability (`--no-truncate` / `-n`), multi-step sub-agent research pipelines, and branch-wide web-search grounding control (`--wslinks` / `AICHAT_WSLINKS`).
**Agent Loop Coverage:** unit-test (`cargo test`) line coverage of `src/agent_loop.rs` rose **46.8% → 64.7%** (+17.9 pts) from #5 — see [coverage re-measurement 2026-09-02](coverage-remeasurement-2026-09-02.md). NB: not comparable to the older 72.9% figure (live E2E harness, different methodology — [2026-08-31 report](coverage-evaluation-2026-08-31.md)).

## Status Table (canonical)

**This is the single source of truth for each item's status.** Full item detail is in [View 3](#view-3--backlog-item-detail); strategic mapping in [View 1](#roadmap--item-crosswalk).

| # | Feature / Item | Status | Pri. | Val. | Effort | LHF | Notes / Relationship |
|:---:|:---|:---|:---|:---|:---|:---:|:---|
| | **▼ MERGED & SHIPPED** | | | | | | |
| **1** | **Rust MCP Bridge** | 🟢 Done | 🔥 High | 💎 High | 🏋️ Large | | Foundation |
| **3** | **Client-Side Agent Loop** | 🟢 Done | 🔥 High | 💎 High | 🏋️ Large | | Core fork thesis |
| **6** | **Tool Safety Modes (umbrella)** | 🟢 Done | 🔥 High | 💎 High | 🎒 Multi | | — |
| 6a | ↳ Deterministic capability mask | 🟢 Done | 🔥 High | 💎 High | 🚶 S-M | 🎯 | *(Retroactively LHF)* |
| 6b | ↳ Blast-radius tiers + reversibility | 🟢 Done | 🔥 High | 💎 High | 🚶 Med | | — |
| 6c | ↳ `%assess-risk%` LLM evaluator | 🟢 Done | 🔥 High | 💎 High | 🚶 Med | | — |
| 6d | ↳ Escalation & HITL protocol | 🟢 Done | 🔥 High | 💎 High | 🏋️ Large | | — |
| **16**| **Web-Search Grounding Control** | 🟢 Done | 🔥 High | 💎 High | 🚶 S-M | 🎯 | *(Retroactively LHF)* |
| **4** | **Tool Output Routing** | 🟢 Done | ➖ Med | 🔹 Med | 🚶 Med | | — |
| **5** | **Test Suite & Coverage Hardening** | 🟢 Done | ➖ Med | 💎 High | 🚶 Med | | — |
| **15**| **Plan-Driven Execution** | 🟢 Done | ➖ Med | 💎 High | 🚶 Med | | Consumes #6c |
| **17**| **Progressive Disclosure Skills** | 🟢 Done | ➖ Med | 💎 High | 🚶 Med | | — |
| **19**| **Autonomy Ladder** | 🟢 Done | 🔥 High | 💎 High | 🏃 Small | 🎯 | 🔧 Enhances #6 + #6d. Default `--autonomy readonly` (Method 2); opt out via `none`. |
| **18**| **Host SRE Telemetry & Multi-Agent Triage** | 🟢 Done | 🔥 High | 💎 High | 🏃 Small | 🎯 | 🆕 Shipped in `innators` + `perry` (5-Pillar suite, `host_env`, `sre` agent, parallel orchestration) |
| &nbsp; | | | | | | | |
| | **▼ PROPOSED (TO DO)** | | | | | | |
| **7** | **Session WAL Journaling** | 🟡 Prop. | 🔥 High | 💎 High | 🏋️ Large | | — |
| **8** | **Dynamic Context Compaction** | 🟡 Prop. | ➖ Med | 🔹 Med | 🚶 Med | | — |
| **9** | **Ephemeral Git Worktrees** | 🟡 Prop. | ➖ Med | 🔹 Med | 🚶 Med | | — |
| **10**| **Staged Config & Dry-Run** | 🟡 Prop. | ➖ Med | 💎 High | 🚶 S-M | 🎯 | — |
| **12**| **Remote MCP Transports** | 🟡 Prop. | ➖ Med | 💎 High | 🚶 Med | | Enables L4 control planes |
| **14**| **Per-Machine Audit Log** | 🟡 Prop. | ➖ Med | 🔹 Med | 🚶 Med | | Shares writer with #7 |
| **20**| **Zero-Token Pre-Flight Triage** | 🟡 Prop. | ➖ Med | 💎 High | 🏃 Small | 🎯 | 🆕 Entirely new |
| **21**| **In-Loop Clarification (`ask_user`)** | 🟡 Prop. | ➖ Med | 💎 High | 🏃 Small | 🎯 | 🔧 Enhances #6d |
| **22**| **Deferred Tool Loading** | 🟡 Prop. | ➖ Med | 🔹 Med | 🚶 Med | | 🔧 Enhances #8 + #17 |
| **11**| **Mock-Client Test Seam** | 🟡 Prop. | 🧊 Low | ◽ Low | 🚶 Med | | Extends #5 |
| **13**| **Scoped Shared Artifact Store** | 🟡 Prop. | 🧊 Low | ◽ Low | 🚶 Med | | Interacts #7, #9 |
| **23**| **Flight Recorder & Replay** | 🟡 Prop. | 🧊 Low | 🔹 Med | 🏃 Small | | *(Blocked by #7 & #14)* |
| **24**| **Read-Only Batch Actuation** | 🟡 Prop. | 🧊 Low | 🔹 Med | 🚶 Med | | ⚡ Overlaps #3 + #15 |
| **25**| **Lightweight Episode Store** | 🟡 Prop. | 🧊 Low | 🔹 Med | 🚶 Med | | 🔧 Enhances #13 |
| &nbsp; | | | | | | | |
| | **▼ DEFERRED** | | | | | | |
| **2** | **Gemini Interactions API** | 🔴 Def. | 🧊 Low | ◽ Low | 🏋️ Large | | Covered by OpenRouter |

Legend: 🟢 Done · 🟡 Proposed · 🔴 Deferred. LHF (Low Hanging Fruit) = 🎯 (High/Med Value, S/S-M Effort, Unblocked).

**Effort scale:** S ≈ under ~150 lines / a few hours · M ≈ ~150-400 lines / 1-2 days · L ≈ ~400+ lines or new modules / multi-day. Pillar/Tenet references map to [`fork-philosophy-and-architecture.md`](fork-philosophy-and-architecture.md).

## Commit History

### `feat/tool-output-routing` (off `feat/agent-loop-enhancements`)
1. `8d3f921` — feat: switch default PDF loader to pdf2md (structured Markdown)
2. `d3c9423` — docs: add spec for backlog #4 — Tool Output Routing
3. `9b0794a` — feat: tool output routing — capping, file destination, pipe chains
4. `b5f1914` — docs: update README, architecture, and progress for backlog #4
5. `9fc8817` — docs: add enhancements-demo.md — copy-paste examples for all features
6. `5acb23b` — feat: observability hardening, circuit breaker, cost tracking
7. `fb5e011` — feat: update models, refine agent dispatch, token tracking & add architecture docs
8. `24b7912` — docs: add session summaries, coverage report, and backlog updates
9. `e71b2bc` — docs: add rationale column to backlog status; fix auto-cap threshold
10. `1490cbc` — test: update stale Gemini catalog guardrail; refresh progress

### `feat/agent-loop-enhancements` (off `feat/rust-mcp-bridge`)
1. `65ef41c` — feat: agent loop Phase A — config, module skeleton, async eval, raw LLM call
2. `aeff42b` — feat: agent loop Phase B — iterative loop replaces recursion, turn budget enforced
3. `012a7a2` — docs: add .kiro project docs, specs, and steering
4. `ae68429` — feat: agent loop Phase C — parallel tool execution
5. `45ed50d` — docs: update architecture and progress for Phase A-C completion
6. `6b46a38` — docs: add Fork Enhancements section to README
7. `c48f0d5` — feat: agent loop Phase D — observability and progress rendering
8. `841e582` — feat: agent loop Phase E — planning tool and sub-agent subprocess
9. `3cbc2aa` — docs: enrich architecture and README with design philosophy
10. `73ca1a5` — feat: agent loop Phase F — polish, --info display, tests

### `feat/rust-mcp-bridge` (off `main`)
1. `3e95825` — feat: add native Rust MCP bridge (replaces Node.js bridge)

### `feat/tool-safety-6a` → `feat/tool-safety-6b` → `feat/tool-safety-6c` → `feat/tool-safety-6d` (off `main`, #6 umbrella)
- `c28dfd5` — docs: add spec for backlog #6 (umbrella #6a–#6d)
- `8994922` — feat: backlog #6a — deterministic tool safety capability mask
- `e338c8b` / `6b4e703` — docs: Session 4 handoff (+ #6d WebSocket redesign, #14 audit log)
- `e6c8af2` — docs: revise #6d to mTLS-WebSocket channel; add audit-log #14
- `d8391af` — docs: fix residual file-rendezvous refs in #6 design
- `7de3291` — feat: backlog #6b — blast-radius tiers, proven reversibility, protected policy, authority ceiling
- `a3e8eb6` — feat: #6b follow-ups — delegation-not-gated, catastrophic hard floor, safety env overrides, blocked-trace fix, gate demos
- `e530f95` — docs: consolidate roadmap/progress/backlog; add Session 5 handoff; sync #6b as-built
- `58af926` — feat: backlog #6c — %assess-risk% LLM risk evaluator (stricter-only overlay)
- `d40bccc` — feat: backlog #6d (part 1) — mTLS escalation channel transport + auth core
- `208f31c` — docs: add #6d part 2 handover (escalation loop integration)
- `f57a7e6` — feat: backlog #6d (part 2) — persistent per-process mTLS connection & escalation integration
- `4930c98` — feat: %assess-risk% code-aware safety evaluator and semantic parameter inspection
- `2a7da73` — feat: Option B pre-flight remediation, reversibility clamp fix & live safety demos
- `b6c4b26` — feat: supervisory policy enforcement & risk assessment in escalation (The Should Gate)
- `e64d146` — feat(safety): implement permission vs authorization boundary and bounded re-delegation (FR-6d.18-21)
- `1f701f6` — feat(observability): add agent hierarchy indentation, color coding, payload truncation, and demo descriptions
- `c12890e` — feat(observability): channel dialog blocks through AgentLoopEvent for strict FIFO ordering
- `de0f501` — feat: eliminate downward permits and enforce hard child authority ceilings (FR-6d.24)
- `45d3fde` — docs: finalize Session 16 verification in roadmap and session summary
- `409f973` — docs: define FR-6d.25 hierarchical guide rails, semantic role badging, and turn delta observability
- `362dc8f` — feat: hierarchical guide rails, semantic role badging, and turn delta observability (FR-6d.25)
- `9917ef1` — feat: ANSI-aware soft-wrapping, guide rail continuity, and responsive dialog borders (FR-6d.25)
- `d7568be` — feat(safety): governance grammar, debug journaling, turn loop fix, and resilience (FR-6d.26-27)
- `5522b86` — feat(nano): nanoworker traceability via @meta nano and British Humour petnames
- `1d85794` — feat(nano): inherit parent instance petname with sequence counter for nanoworkers
- `34b8bb2` — feat(petnames): shorten British humor adjectives and nouns for compact petnames
- `f4abd47` — feat(trace): widen indentation, add inheritable ephemeral agent colors, error & escalation styling
- `192fe24` — feat(dialog): color all role keywords, dim historic corpus, and dim response blockquotes
- Companion (`llm-functions` repo, branches `feat/tool-safety-permission-boundary` & `feat/fetch-url-native-html-to-markdown`):
  - `734a376` — feat: expose permissions contract in orchestrator schema
  - `1447f17` — feat(researcher): enable multi-step research pipeline via web_search --links and summarize_text -S
  - `79ede0f` — feat(fetch_url_via_curl): migrate to native html-to-markdown --url with -p --preset aggressive
  - `15bd96a` — feat(fetch_url_via_curl): add --skip-images to omit image elements and data URIs
  - `4adf72e` — feat(meta): add @meta nano support to build scripts and tools

### `feat/structured-plan-and-skills` (off `main`, #15 & #17)
- `dc27f01` — docs(specs): add specifications for #15 (structured plan) and #17 (skills)
- `654657b` — feat(agent-loop): add structured _plan support, plan-state tracking, and risk pre-pass (#15)
- `66199b4` — feat(skills): implement progressive skill registry, read_skill, and provenance-based taint (#17)

## What's Implemented

### #1 Rust MCP Bridge ✓
In-process Rust MCP replacing Node.js. Transparent integration (Option C), cached manifests, lazy spawn, feature-flagged.

### #3 Agent Loop Enhancements ✓
Provider-agnostic iterative loop with: parallel tool execution (semaphore-bounded `join_all`); turn budget (configurable `max_turns`, stderr warning); planning tool (`_plan` auto-injected, acknowledged result, trace events); sub-agent subprocess delegation (aichat spawns aichat, depth-bounded); progress rendering (spinner + trace, 2s heartbeat); external observability (OSC title, JSON status file, BEL + OSC 777). 29 tests.

### #4 Tool Output Routing ✓
Declarative output routing on `FunctionDeclaration`: auto-capping (results > `tool_output_limit` → temp file + preview); file destination (write to path template, return confirmation); pipe destination (chain tools without LLM round-trip, cycle detection). 16 tests.

### #6a Deterministic Capability Mask ✓ (unmerged)
First increment of the #6 umbrella: `ToolMode { Readonly, Mutating }` + skip-serialized `mode` field; `SafetyClass { Readonly, Mutating, Unclassified }` (absent mode = Unclassified, reserved to humans). Sub-agents inherit `AICHAT_CAPABILITY_MASK=readonly`; masked processes refuse mutating/unclassified tools with a structured `capability_denied` result (not a crash). MCP tools → unclassified; `_plan` always permitted. +8 tests. The permanent safety floor #6b–#6d degrade back to.

### #6b Blast-Radius Tiers, Reversibility, Policy & Ceiling ✓ (unmerged)
Second increment — graduated deterministic governance (no LLM):
- 5-tier `BlastRadius` (`Safe`<`Reversible`<`Disruptive`<`Destructive`<`Catastrophic`, `Ord`) + `risk`/`reversible`/`reversible_via` skip-serialized fields; `StaticTier` resolver (explicit `risk` wins; legacy `mode` maps `readonly`→`Safe`, `mutating`→`Disruptive`; else Unclassified).
- New `src/safety.rs`: pure `required_authority(tier, policy, proven_reversible)` (proof lowers one step; unclassified/forbid → Human); `AuthorityCeiling`; `PolicyFile` (owner-only YAML, raise-or-forbid, hand-rolled glob, strictest-match).
- Top-level `safety:` config (`policy_file`, `risk_model`, `default_ceiling`=Destructive, `escalation_dir`, `verdict_timeout_secs`).
- Two deterministic dispatch gates in `eval_single_tool`: over-ceiling → `authority_exceeded`, policy → `policy_forbidden`; `AICHAT_AUTHORITY_CEILING` propagated to children (parent may only lower). Reserved `EscalationMsg`/`VerdictMsg` WS schemas for #6d.

**Follow-up hardening (same branch, driven by validating the live demo harness):**
- **Decision B — delegation is not gated.** A call targeting a sub-agent (`call_targets_agent`) skips *both* the #6a and #6b gates: delegating is orchestration, and the child's own actions are gated inside its process. Without this, unclassified agent-tools would be human-reserved and multi-agent mode off-by-default.
- **Catastrophic hard-floor clamp.** Proven reversibility no longer discounts a `Catastrophic` tier (`base != Catastrophic` guard in `required_authority`) — catastrophic always requires a human, so a tool's `reversible: true` flag can't undercut a policy-imposed catastrophic raise.
- **`ToolBlocked` loop event + `safety_block_reason()`** — a gate-denied call traces as `<tool> BLOCKED (<reason>)` (not the misleading `completed`) and skips output routing; the tool binary never runs.
- **`AICHAT_SAFETY_POLICY_FILE` / `AICHAT_SAFETY_DEFAULT_CEILING`** env overrides (match the `AICHAT_AGENT_LOOP_*` pattern) in `config::load_envs`.
- **All 31 stock tools classified** (companion work in the `llm-functions` repo, branch `feat/tool-safety-classification`): `# @meta risk <tier>` annotations + `build-declarations.{sh,js,py}` extended to emit `risk`/`reversible`. Unclassified tools are human-reserved (why classification was needed for the engine to be usable by default).
- +33 unit tests (incl. catastrophic clamp, `safety_block_reason`); suite 360→385 unit, 0 fail; clippy no new warnings. Live gate demos 13–15 pass on `gemini-2.5-flash`.

### #6c %assess-risk% LLM Evaluator ✓ (unmerged)
Third increment — dynamic risk evaluation overlay:
- Dedicated role `assets/roles/%assess-risk%.md` using cheap model `safety.risk_model` (e.g. `gemini-2.5-flash`).
- **Enriched Semantic & Implementation Context:** The evaluator inspects underlying script code (checks MCP, agent `tools/`/`bin/`, root `tools/`/`bin/`, traverses runner symlinks, detects binary files via null-byte scanning, enforces 4KB text budget sliced at UTF-8 boundaries), semantic header docstrings (`@describe`, multi-line notes, parameter docs, `@env`), and a formatted CLI invocation preview.
- **Code-Aware Evaluator Reasoning:** Evaluator instructions direct the model to contrast declared `@describe` intent against script implementation, trace argument flow into sinks (`eval`, `rm`, shell interpreters, curl), detect hardcoded destructive operations, and verify confirmation guards (`guard_operation.sh`).
- **Execution-Level Ground Truth Inspection (`FR-6c.10`):** Replaces `"type": "unknown"` fallback by parsing multi-tool scripts (e.g. `agents/<agent>/tools.sh`) via `extract_shell_function` with balanced brace/quote parsing and `@cmd`/`@describe` header extraction. Evaluator prompt strips OpenAPI schema noise (`permissions_*`, `__*`) and inspects concrete bash execution code.
- **Verdict-Level Caching (`FR-6c.9`):** `RiskCache` caches full `RiskVerdict` (tier, reversible, confidence, rationale, concerns) rather than a scalar floor enum. Preserves evaluator rationale for trace previews (`(floor: ...): "..."`) and denial errors, enables dynamic act-time reversibility discounting via `clamp_verdict`, and maintains monotonic safety floors.
- **Stricter-only clamp:** Rust clamps evaluator verdict such that it can only *raise* risk or *withhold* reversibility credit; never relaxes static tiers or policy rules ("the LLM is not a Pardoner").
- **`Safe` fast-path:** Read-only / `Safe` actions bypass the evaluator entirely.
- **Monotonic raise-only `RiskCache`:** Reuses assessments for identical tool calls within a run without ever green-lighting or downgrading.
- **Complete Parameter Classification (100%):** All 31 root tools and all agent subcommands (`coder`, `demo`, `json-viewer`, `orchestrator`, `researcher`, `sql`, `todo`) classified with `mode`, `risk`, and reversibility metadata (`83dea2e`).
- +24 unit tests + context inspection tests; graceful degradation when `risk_model` is unspecified.


### #6d Escalation & Control Protocol + HITL ✓ (unmerged)
Fourth increment — persistent mTLS control plane, human-in-the-loop, durable rollback journaling, Option B opportunistic remediation, and downward supervisory propagation:
- **Persistent Per-Process mTLS Client (`ChildEscalationClient`):** Sub-agents establish a single persistent connection to parent's loopback mTLS listener, multiplexing `Hello` → `Events` → `Escalations` ↔ `Verdicts` → `Results`/`Errors`.
- **Actor-Isolated Single Writer:** Dedicated Writer task exclusively owns `WriteHalf`, preventing byte interleaving and framing desync.
- **Demuxed Oneshot Reader & Immediate Fail-Closed:** Reader task owns `ReadHalf` and routes inbound verdicts to waiting callers via an in-memory `oneshot` registry. If socket drops or parent dies, all pending oneshots immediately drain with a fail-closed error.
- **Event Backpressure:** Bounded 1024-element MPSC with `try_send` drop-newest on saturation for non-blocking child progress events; parent renders child events live as `[child <agent_id>] ...`.
- **Durable Rollback Journal (0600):** Append-only on-disk `RollbackJournal` under `$XDG_RUNTIME_DIR/aichat/journals/` with atomic replay on `Revert` verdicts.
- **Option B (Pre-flight Opportunistic Remediation):** When an autonomous tool call trips an agent's authority ceiling solely because it is not yet proven reversible, and declares `reversible-via backup` (e.g. `fs_write`), the engine creates an atomic pre-mutation file backup (or `rm -f '<path>'` undo command for new files) in the durable rollback journal upfront. This steps down required authority (`one_step_down(Disruptive) = Reversible`), permitting autonomous actuation.
- **Reversibility-Aware Verdict Clamping:** `clamp_verdict` accepts `reversible: bool` and steps down the evaluator's raw risk assessment (`one_step_down(verdict.tier)`) before clamping with `stricter_of`, preserving the discount when the evaluator agrees with the tool's declared blast radius.
- **Evaluator Context Awareness:** Evaluator payload includes `"rollback_mechanism": "atomic pre-mutation backup in durable rollback journal"`.
- **Elimination of Downward Permits & Strict Child Authority Ceilings (`FR-6d.24`):** Complete elimination of downward execution permits (`token`, `supervisory_approved`). `VerdictMsg` stripped of pass-through tokens. A supervisor cannot override a child's authority ceiling. When an action exceeds the child's ceiling, actuation is blocked immediately (`authority_exceeded`), pre-mutation journal entries unwind cleanly, and structured block information returns to the parent orchestrator for bounded re-delegation.
- **Permanent Model Definition in Role:** `%assess-risk%.md` front-matter supports direct `model:` definition.
- **Unified Scannable Governance Nomenclature (`FR-6d.26`, `FR-6d.27`):** Trace events follow a scannable grammar (`<VERB> <tool>: <lhs> <op> <rhs>`) with `risk` always on LHS and `ceiling` on RHS (`ALLOW <tool>: risk <tier> <= ceiling <tier>` vs `BLOCK <tool>: risk <tier> > ceiling <tier>`), with pure single-source mechanism derivation (`format_risk_token`) producing parenthetical `(effective, <why>)` qualifiers. Interactive banner rendered as `[HUMAN APPROVAL REQUIRED] <tool>` with threaded authority ceiling.
- **Human-in-the-Loop CLI UX:** Interactive single-key terminal prompt (`[c]ontinue | [h]alt | [r]evert | [e]xplain | [g]uide`); headless Layer-3 fail-closed mode.
- **Nanoworker Traceability & British Humour Petnames:** Ephemeral utility tools marked with `@meta nano true` inherit parent petname with sequence counter (`nano-<ParentPetname>-<Seq>`); all petnames drawn from compact 32-entry British humour dictionaries (avg 10.8 chars).
- **Indentation, Guide Rails & Ephemeral Colors:** Widened ancestor visual rails to 6 columns (`│     `), indented trace lines by 4 spaces, 11-color ANSI palette with `AICHAT_AGENT_COLOR` inheritance, soft coral `ERROR_COLOR` (`#e06c75`), and warm amber `ESCALATION_COLOR` (`#d19a66`).
- **Semantic Dialog Role Coloring & History Dimming:** Distinct semantic coloring for `[user]`, `[assistant]`, `[system]`, `[history: <role>]`, and `[tool]`, with Dark Gray historic corpus dimming, `⚡ [new: ...]` delta highlighting, and dimmed response blockquotes.
- **Live Demos 17–20:** Demos 17 (Happy Path autonomous write), 18 (Option B Pre-flight Remediation), 19 (Ceiling Fail-Closed), 20 (Orchestrator to Coder Multi-Process Escalation).
- +37 unit/integration tests (506 total workspace tests); Demo 16 offline fail-closed and journal durability verified.

### #15 Serious Structured `_plan` / Plan-Driven Execution ✓ (unmerged)
- **Structured Plan Schema & Dual-Arm Parser:** Published formal schema with typed `objective`, `steps: [{ id, intent, tool, args_preview, depends_on }]`, and `thought`. Resilient `PlanPayload` deserializer parses structured objects while gracefully accepting legacy string or partial object payloads (`{"thought": "..."}`) without dropping context or erroring.
- **Plan State Tracking & Live Progress Events:** State tracker (`PlanTracker`) provides `start_step`, `complete_step`, `fail_step`, and human-readable plan summaries. Emits `PlanReceived` and `PlanStepUpdated` events.
- **Ahead-of-Time Risk Pre-Pass (`plan_risk_prepass`):** Evaluates all planned tool calls in advance against `#6c`'s `%assess-risk%` evaluator, populating the monotonic, raise-only `RiskCache`. Emits `PlanRiskPrepassFlagged` on risky steps. Degrades cleanly to a no-op when `#6c` is absent.

### #17 Progressive Disclosure Runbooks & Skills (`read_skill`) ✓ (unmerged)
- **Progressive Discovery & 3-Tier Precedence:** Discover skills from workspace roots (`.kiro/skills`, `.agents/skills`, `.skills`), global config (`$XDG_CONFIG_HOME/aichat/skills`), and system builtins with workspace > global > builtin precedence.
- **YAML Frontmatter Parser:** Lightweight YAML frontmatter parser for `SKILL.md` (no mandatory SHA/hash verification, zero developer friction).
- **Agent Eligibility & Nano Exclusion:** Agent configs specify `skills: all | false | [...]`. Nano utility workers (`@meta nano true` / `nano: true` / `AICHAT_AGENT_NANO=true`) strictly receive an empty catalogue and no `read_skill` tool. Eligible agents receive an injected `### Available Skills` metadata catalog in their system prompts.
- **Dynamic Tool Injection & Execution:** Automatically registers `read_skill(name)` which executes and returns instruction text, description, path, and provenance.
- **Step-Scoped Active Taint Lifecycle (`ActiveSkillTracker`):** Loading a workspace-tainted skill sets `untrusted_runbook: true` and tracks `active_tainted_skills`. Taint is bound to the active plan step and cleared immediately upon step completion.
- **Evaluator Integration & Prepass Simulation:** Added heightened scrutiny Directive 5 to `%assess-risk%` without pardoning authority. Simulated planned skill loads in `plan_risk_prepass` to flag downstream steps in advance.

### PDF Loader Enhancement
Default `document_loaders.pdf` switched from `pdftotext` to `pdf2md --compact --raw` (firecrawl/pdf-inspector). Structured Markdown for better RAG chunking and token efficiency.

## Architecture Decisions Log

| Decision | Rationale |
|----------|-----------|
| Don't merge server-side and client-side agent loops | Different delegation models. Shared tool execution layer, separate orchestration. |
| Iterative loop (not recursive) | Trivial budget enforcement, no stack growth, natural progress reporting. |
| Sub-agents as subprocess (not in-process) | Each agent gets its own PID, status file, observability. Process boundary enables crash isolation and future Model B. |
| Parallel by default | Single-tool turns have zero overhead. Multi-tool turns get automatic speedup. |
| MCP pool spawns extra connections for parallel | Simpler than a connection queue. MCP servers are lightweight. |
| Tool output handles (capping) | Prevents context blowout from large tool results. Full content accessible via temp file path. |
| Declarative output routing | Tools declare destinations; LLM never sees routing config (skip_serializing). |
| Pipe chains with cycle detection | Enables tool composition without LLM round-trips. Acyclic guarantee. |
| OSC title + status file + bell | Makes aichat observable by tmux, Herdr, Agent Deck without custom integration. |
| Skip Gemini Interactions API | OpenRouter proxies Gemini. The client-side loop makes this sufficient. |
| pdf2md over pdftotext | Structured Markdown preserves headings/tables for RAG. 30-40% fewer tokens. |
| Tools are classified, engine isn't loosened (#6b) | We own the tools, so declare each one's blast-radius tier via `@meta` rather than defaulting unclassified tools to permissive. Keeps "unclassified → human-reserved" strict (fires only for truly-unknown/MCP tools). |
| Delegation is not gated (decision B, #6b) | Spawning a sub-agent is orchestration, not actuation; gating it double-counts and breaks multi-agent mode. The child's own actions are gated inside its process (mask + ceiling). |
| Catastrophic is a hard human-only floor (#6b) | Proven reversibility must not discount catastrophic, or a tool-author-set `reversible: true` could undercut a policy-imposed catastrophic raise. Catastrophic always → human. |
| Blocked tools trace as BLOCKED, not completed (#6b) | A gate denial returns via the dispatcher's Ok path; a distinct `ToolBlocked` event keeps the trace truthful (the binary never ran) and skips output routing. |
| The LLM is not a Pardoner (#6c) | Evaluator verdicts can only raise risk or withhold reversibility; never relax static tiers or policy rules. |
| Raise-only RiskCache (#6c) | Caching authority requirements across turns prevents redundant model calls without risking permissive bypass. |
| Implementation source inspection in risk evaluator (#6c) | Evaluator model previously evaluated tool names and args blindly without knowing what the script executed (e.g. eval vs safe logic). Passing bounded 4KB script source + semantic comments allows accurate risk tracing while the stricter-only clamp guarantees untrusted code/args can only raise risk, never pardon. |
| Path 1′ in-tree mTLS + length-delimited JSON (#6d) | Builds mTLS over in-tree `tokio-rustls` with big-endian framing without adding heavy WebSocket crate dependencies. |
| Persistent per-process mTLS connection (#6d) | Single handshake per sub-agent lifecycle carrying events, escalations, and results eliminates connection churn. |
| Single-writer actor serialization (#6d) | Exclusive ownership of `WriteHalf` in a background actor prevents byte interleaving across concurrent events/escalations. |
| Demuxed oneshot reader + immediate fail-closed (#6d) | Reader task demuxes verdicts by `escalation_id` and instantly fails pending oneshots if the socket closes. |
| Drop-newest bounded event channel (#6d) | 1024-element MPSC with `try_send` prevents event backpressure from stalling model execution or exhausting memory. |
| Option B pre-flight opportunistic remediation (#6d) | Solves the chicken-and-egg gating paradox where tools declaring `reversible-via backup` were blocked before reaching the actuation point where backup is created. Upfront journal backup steps down required authority to `one_step_down(tier)`, permitting autonomous execution within ceiling. |
| Reversibility-aware verdict clamping (#6d) | When a tool is proven reversible, stepping down the evaluator's raw verdict (`one_step_down(verdict.tier)`) before clamping with `stricter_of` preserves the reversibility discount when evaluator agrees with tool tier, preventing monotone clamp from accidentally erasing the step-down. |
| Permanent model definition in role front-matter (#6c/#6d) | `AgentConfig::load_role` honors `model:` declared in `%assess-risk%.md` front-matter, avoiding required manual config file edits when configuring a dedicated evaluator model. |
| Supervisory policy enforcement & risk assessment ("The Should Gate", #6d) | Having authority does not mean the supervisor should authorize an action ("can != should"). Supervisor independently loads its own Protected Policy File, computes an anti-spoofed static tier and reversibility floor, evaluates proposed child actions using %assess-risk% with extended supervisory context (child ID, depth, stated reason, enrichment), clamps strictly, and fails toward safety (Human) on Low confidence or error. |
| Verdict-level caching in RiskCache (#6c, FR-6c.9) | Caching full `RiskVerdict` rather than scalar `RequiredAuthority` preserves evaluator rationale across cache hits and enables dynamic act-time reversibility re-evaluation while maintaining non-pardonable monotonic floors. |
| Execution ground truth script function extraction (#6c, FR-6c.10) | Multi-tool scripts group agent subcommands under single files (`tools.sh`). Extracting the target function body via `extract_shell_function` lets the risk assessor inspect actual bash logic rather than evaluating blind metadata envelopes or falling back to "unknown". |
| Elimination of downward permits & hard child authority ceilings (#6d, FR-6d.24) | Downward pass-through execution tokens (`ExecutionPermit`) and supervisor permit propagation were eliminated. A child's authority ceiling is a hard boundary that cannot be bypassed by parental tokens. When required authority exceeds child ceiling, child unwinds and blocks (`authority_exceeded`); parent orchestrator re-delegates with higher ceiling upfront (bounded to 2 attempts) or actuates directly. |
| Ephemeral British humour petnames & nanoworker inheritance (#6d, Session 21) | Sub-agent PIDs are rendered as compact, deterministic British humour petnames (avg 10.8 chars) via dual 32-bit integer mixes. Ephemeral nanoworker utility tools marked `@meta nano true` inherit the parent's petname (`nano-<ParentPetname>-<Seq>`) via environment propagation to distinguish disposable tool workers from autonomous sub-agents. |
| Ephemeral inheritable agent color palette & dialog dimming (#6d, Session 21) | 11 soft ANSI colors derived deterministically via `djb2_hash(petname)` are inherited via `AICHAT_AGENT_COLOR`, maintaining a consistent visual identity for each agent tree. Dialog frames feature distinct role keyword coloring, Dark Gray historic corpus dimming, white active turn delta headers, and dimmed Markdown blockquotes in LLM responses. |
| Branch-exclusive agent color hierarchy & nano caller inheritance (#6d, Session 23) | Hierarchical palette stratification where root orchestrator is exclusively Cyan, direct subagents cycle through 10 mutually exclusive colors via an atomic sequence counter (passed via `AICHAT_AGENT_COLOR` and `AICHAT_SUBAGENT_SEQ`), sub-subagents are offset into upper partitions, and nano-subagents inherit their invoking caller's color. Zero filesystem locks or tmp files. |

## Branch Status


- `main` — upstream fork at v0.31.0-fork.9 (all prior `feat/*` branches merged here)
- `rc-branch` — release candidate (stale ancestor of `main`)
- `feat/rust-mcp-bridge` — #1, complete (merged to `main`)
- `feat/agent-loop-enhancements` — #3, complete (merged to `main`)
- `feat/tool-output-routing` — #4, complete (merged to `main`)
- `feat/test-suite-hardening` — #5, complete (merged to `main`)
- `feat/tool-safety-6a` — #6a, complete (folded into `feat/tool-safety-6b`'s history)
- `feat/tool-safety-6b` — #6b, complete (folded into `feat/tool-safety-6c`'s history)
- `feat/tool-safety-6c` — #6c, complete (folded into `feat/tool-safety-6d`'s history)
- **`feat/tool-safety-6d`** — #6d, **implemented & verified (active, unmerged; commit `2a7da73`)**
- Companion: `llm-functions` repo, branch `feat/tool-safety-classification` — all 31 tools classified (unmerged)

## Merge Strategy

```
main ← feat/rust-mcp-bridge ← feat/agent-loop-enhancements ← feat/tool-output-routing (all merged)
main ← feat/tool-safety-6a ← feat/tool-safety-6b ← feat/tool-safety-6c ← feat/tool-safety-6d (current, unmerged)
```

## Environment Reminders

- Production aichat: `/usr/bin/aichat` (v0.30.0), config at `~/.config/aichat/`
- Active Perry dev binary: `~/projects/perry/target/release/perry` (or debug build `target/debug/perry`)
- To test: `AICHAT_CONFIG_DIR=~/.config/perry` (or `/tmp/perry-test`)
- Active actuator functions: `~/projects/innators` (`AICHAT_FUNCTIONS_DIR=~/projects/innators`)
- Upstream reference clones: `~/clones/aichat` and `~/clones/llm-functions`
- pdf2md: installed via `cargo install pdf-inspector`

---
---

# View 3 — Backlog (item detail)

Full detail per item. **Status is in the [Status Table](#status-table-canonical), not here.**

> Items #5-#10 originate from the Session-2 architecture blueprint (§5, "six high-leverage Layer-2 capabilities").

## 1. Rust MCP Bridge

**Spec:** `.kiro/specs/rust-mcp-bridge/` (requirements, design, tasks). Committed `3e95825`; 1272 lines / 9 files.

Replaced the Node.js MCP bridge with an in-process Rust implementation. MCP-sourced tools appear identical to shell-exec tools — no new abstractions, two integration points (`ToolCall::eval()` routing + `Config::load_functions()` discovery). Cached tool manifests for fast startup, lazy server spawn on first invocation, graceful shutdown on exit. Behind `mcp` cargo feature flag (default on).

## 2. Gemini Interactions API Module

**Driver:** Google's `generateContent` endpoint is labelled "legacy" since June 2026. The Interactions API is GA, supports both Gemini 2.x and 3.x models uniformly, and maps almost 1:1 to the pattern already established in `openai_responses.rs`. *(Deferred — Gemini agentic use is already covered via OpenRouter + the provider-agnostic client-side loop #3.)*

### Approach
Create a self-contained module following the `openai_responses.rs` template. This does NOT replace `gemini.rs` — both coexist, with routing based on model capability or user preference.

```
gemini.rs              → generateContent (legacy, still works, kept for older models)
gemini_interactions.rs → Interactions API (new, agentic, streaming)
vertexai.rs            → Routes to either based on model/config
```

**What to implement:** (1) Session lifecycle — `POST /interactions` to create, typed `steps[]`. (2) Step types — map `thought`/`model_output`/`function_call`/`function_result` to `ChatEvent` variants. (3) Tool-use loop — on `requires_action`, execute locally, send results, continue (same as `openai_responses.rs`). (4) Streaming — SSE `step.start`/`step.delta`/`step.stop` → `SseEvent::Text`/`Reasoning`. (5) Built-in tools — `google_search`, `code_execution` as pass-through. (6) Thinking — parse `thought` + `thought_signature`. (7) Auth — reuse Gemini API key + VertexAI OAuth.

**What this solves:** eliminates the Gemini 2.x vs 3.x branching; Gemini parity with OpenAI agentic capabilities; future-proofs against deprecation; native tool-use loops without server-side orchestration.

**References:** Interactions API spec `https://ai.google.dev/api/interactions`; template `src/client/openai_responses.rs`; current legacy `src/client/gemini.rs`, `src/client/vertexai.rs`.

## 3. Client-Side Agent Loop Enhancements

**Driver:** The classic agent loop (`run_directive`) is the only provider-agnostic orchestration. It works with anything returning tool_calls but was too primitive for multi-step work. Enhancing it gives every provider agentic capabilities without server-side support.

### Approach
Enhance the client-side recursive loop *without* merging it with server-side multi-agent mode (peers; shared tool execution layer, separate orchestration). Enhancements (by dependency): (1) parallel tool execution (concurrent `join_all`, `ToolCall.id` correlation, serial fallback for side-effects); (2) max-turns budget (default 20, warning on exhaustion); (3) agent-as-tool sub-agent delegation (`agent: bool` → spawn sub-agent, return output as tool result); (4) progress/trace reporting (generalize `OpenAIResponsesProgress`); (5) optional planning tool (`_plan` scratchpad, appended to next turn's context, not shown to user).

**Does NOT:** merge with server-side multi-agent; add cross-session memory; change tool definition/discovery (#1); add human-in-the-loop pauses (that's #6d).

**Key principle:** server-side loop (OpenAI Responses, future Gemini Interactions) and client-side loop are peers — server-side when the provider supports it, client-side as the universal fallback.

## 4. Tool Output Routing

**Driver:** Every tool result goes back into the LLM's context — wasteful/wrong for large or final outputs. Routing control makes composition practical without burning context.

### Approach
Tools declare where output goes: **context** (default; auto-capped at `tool_output_limit`, default 16KB → temp file + preview); **file** (written to a path template, model gets a confirmation); **pipe** (passed to another named tool without an LLM round-trip; acyclic, cycle-detected).

```yaml
# In functions.json or agent config
[
  { "name": "generate_report", "parameters": { }, "output": { "destination": "file", "path": "/tmp/{{name}}.md" } },
  { "name": "fetch_raw_data",  "parameters": { }, "output": { "destination": "pipe", "target": "summarize_data" } }
]
```

**Solves:** context-window pollution; tool pipelines without per-step LLM round-trips; idiomatic "write a file" tools; lower token cost. **Does NOT:** change tool discovery/definition format (augments with optional field); stream between tools (pipe is batch); affect turn counting (a piped chain = one tool execution). **Dependency:** #3 first (routing integrates into the async parallel dispatch layer).

## 5. Test Suite Expansion & Code Coverage Hardening

**Spec:** `.kiro/specs/test-suite-hardening/`. **Driver:** Dynamic coverage on 2026-08-31 showed a strong baseline (`agent_loop.rs` 72.9% line / 79.4% function under the live harness) but flagged untested edge paths.

### Approach
Targeted unit tests + harness assertions across: (1) **output-routing edges** — cyclic pipe abort, templated-file errors / dir auto-creation / permission failures, auto-cap byte-boundary; (2) **sub-agent isolation** — non-zero exit + stderr capture, depth-overflow boundary, timeout/cancellation propagation; (3) **budget & cost edges** — mid-turn cost-cap exhaustion, partial token tracking across fragmented SSE, circuit-breaker trips; (4) **MCP bridge** — handshake timeouts, malformed JSON-RPC errors. *(Done: +25 tests, suite 327→352; FR-4 via offline Demo 12; circuit-breaker/cost extracted + unit-tested; `agent_loop.rs` 46.8%→64.7% line — see `coverage-remeasurement-2026-09-02.md`.)*

## 6. Tool Safety Modes & Actuation Governance (umbrella)

**Driver:** In system-wide operations, autonomous agents — especially parallel sub-agents on real infrastructure — must never cause catastrophe. The engine enforces *loop* safety (turns, cost, per-tool circuit breaker) but had no notion of an action's *danger* nor any gate on *which agent* may perform it. #6 adds that actuation-governance layer.

> **Scope note:** the original #6 was a ~150-250 line binary capability mask. In design it grew into a layered decision funnel (deterministic policy floor → blast-radius tiers + proven reversibility → stricter-only LLM risk evaluator → escalation/human-in-the-loop). Tracked as an **umbrella** with sub-items #6a–#6d. Each increment is usable alone and **degrades gracefully** to the one below: **#6d escalates → without it #6c/#6b _block_ → without them #6a's binary mask applies.** Full spec: `.kiro/specs/tool-safety-modes/` (see the "As-Built Notes" section for #6b decisions).

### Core principles (invariants across all increments)
1. **The LLM is not a Pardoner** — a risk verdict may only make an action *stricter*, never loosen a deterministic decision.
2. **Deterministic floor first, LLM second** — the policy floor + static classification run before any LLM is consulted.
3. **Blast radius is action-intrinsic; authority grows toward the root** — danger does not correlate with delegation depth, but the autonomous ceiling increases toward the orchestrator (more context up top).
4. **Reversibility must be proven, not asserted** — only real rollback artifacts (tool-intrinsic, or agent-manufactured backup/staging/worktree) count.
5. **Fail toward escalation, not action** — unavailable judgment or over-ceiling escalates; absent an escalation channel it blocks; never silently permitted.
6. **Escalation suspends only the branch** — sibling parallel work continues.

### Increment #6a — Deterministic capability mask (the floor / fallback)
**Scope:** `function.rs`, `agent_loop.rs`, `mcp.rs`
- `FunctionDeclaration` gains a skip-serialized `mode: readonly | mutating` (LLM never sees it).
- Sub-agent subprocesses inherit `AICHAT_CAPABILITY_MASK=readonly` (same channel as `AICHAT_AGENT_DEPTH`); a masked child refuses `mutating`/unclassified tools with a structured `capability_denied` result.
- **Unclassified tools (no `mode`, incl. MCP) are reserved to humans** — stricter than a plain `mutating` default.
- Fully functional standalone; the permanent fallback for all later layers.

### Increment #6b — Blast-radius tiers, proven reversibility, Protected Policy File, authority gradient
**Scope:** new `src/safety.rs`, top-level `safety:` config, `function.rs`, `agent_loop.rs`
- 5-tier ordered blast radius: `Safe < Reversible < Disruptive < Destructive < Catastrophic`.
- **Reversibility is an orthogonal *proven* boolean**, not a tier point; proof lowers the *authority required* by one step, never the radius — **except `Catastrophic`, a hard human-only floor that reversibility cannot discount** (so a `reversible: true` flag can't undercut a policy-imposed catastrophic raise).
- **Protected Policy File** — deterministic, non-pardonable owner-only YAML; rules match tool-name globs + `arg_glob`/`arg_contains` and can only *raise* an action's tier or *forbid* it. `AICHAT_SAFETY_POLICY_FILE` / `AICHAT_SAFETY_DEFAULT_CEILING` env overrides.
- **Root-favoring authority ceiling** propagated via `AICHAT_AUTHORITY_CEILING` (parent may only lower). Over-ceiling → `authority_exceeded`; policy-forbidden → `policy_forbidden`; both **block** (escalation not built yet). Fully deterministic — no LLM.
- **Decision B — delegation is not gated:** a call targeting a sub-agent skips both gates (orchestration, not actuation); the child's own actions are gated inside its process.
- **`ToolBlocked` trace event** — gate-denied calls trace as `<tool> BLOCKED (<reason>)` (not `completed`) and skip output routing; the tool binary never runs.
- **Tools classified:** all 31 `llm-functions` tools declare `# @meta risk <tier>`; `build-declarations.{sh,js,py}` extended to emit `risk`/`reversible` (companion repo `feat/tool-safety-classification`). Reserved typed `EscalationMsg`/`VerdictMsg` WS schemas for #6d.
- **Validated live:** demos 13 (policy forbid), 14 (authority ceiling), 15 (arg-sensitive escalation) in `scripts/run-demos.nu`.

### Increment #6c — `%assess-risk%` LLM evaluator (stricter-only overlay)
**Scope:** `assets/roles/%assess-risk%.md`, `src/safety.rs`, `src/agent_loop.rs`, `safety.risk_model` config
- A new role shaped like `%explain-shell%` returns a **terse structured verdict** for a single action, using a **dedicated cheap model** and bounded structural context (tool + resolved args + static tier + reversibility + this-step intent).
- **Semantic & Implementation Context:** The evaluator inspects underlying script code (checks MCP, agent `tools/`/`bin/`, root `tools/`/`bin/`, traverses runner symlinks, detects binary files via null-byte scanning, enforces 4KB text budget sliced at UTF-8 boundaries), semantic header docstrings (`@describe`, multi-line notes, parameter docs, `@env`), and a formatted CLI invocation preview.
- **Code-Aware Evaluator Reasoning:** Directives instruct the model to contrast declared `@describe` intent against script implementation, trace argument flow into sinks (`eval`, `rm`, shell interpreters, curl), detect hardcoded destructive operations, and verify confirmation guards (`guard_operation.sh`).
- Verdict `{ tier, reversible, confidence, rationale, concerns[], enrichment }` is **clamped stricter-only** in Rust: may raise the tier or withhold reversibility credit; may never loosen or override policy.
- `Safe`/reads skip the evaluator (fast-path). Evaluator backed by a monotonic **raise-only `RiskCache`** (as-built, replacing literal plan-time flagging to prevent unflagged injection skips). Fail/low-confidence ⇒ escalate (block pre-#6d).


### Increment #6d — Escalation & control protocol + human-in-the-loop
**Scope:** `src/safety.rs`, `agent_loop.rs`, `main.rs`/`repl`
- **mTLS WebSocket inter-agent channel (child dials parent).** Parent binds a loopback WSS listener at spawn and passes address + per-child credentials via env (`AICHAT_AGENT_PARENT_ADDR`, `AICHAT_AGENT_TOKEN`, `AICHAT_TREE_SECRET`); child connects back. **Mutual auth** via ephemeral fingerprint-pinned keys (no CA/PKI) + channel-bound challenge–response — forged/injected verdicts are structurally impossible without completing the handshake. Loopback-only binding. No stdin/stdout coupling; ARGC and the earlier file-rendezvous both dropped.
- **Typed message protocol:** child→parent `Hello`/`Event`/`Escalation`/`Result`; parent→child `Verdict{Halt|Revert|Continue}`/`Cancel`. Child hits a gated action, sends `Escalation` (WHY + enrichment + proposed action), and **blocks on `recv()`** (no polling). Verbs execute in the child; **REVERT replays a durable on-disk rollback journal entry** (control plane = the ephemeral connection; durability plane = the journal — reversal survives connection drops / child death / re-spawn).
- Escalation **propagates upward** (child re-escalates up its own connection to its parent), accumulating an evidence trace to the orchestrator; if still undecided, reaches a **human** — a branch-blocking interactive CLI prompt (siblings keep running) **or** the same record emitted to a **Layer 3** supervisor when headless.
- **Liveness is free** from connection state (EOF on either side's death — replaces `/proc` scanning). **Branch-scoped suspension** — a lineage blocked on a verdict never blocks sibling parallelism.
- **Forward-compatible with remote agents:** the same message protocol + auth model generalizes to remote sub-agents (WSS on a routable interface, real certs, agent-registry `endpoint:` field) — not built here, but the protocol must not preclude it.
- **Permission vs. Authorization Separation (The Permission Boundary):** Strict architectural separation between static process capabilities (the #6a capability mask) and supervisory goal-alignment (the #6b/#6c/#6d Should Gate).
  - *Hierarchical Upfront Provisioning:* Orchestrator passes `permissions: { mask, ceiling }` (or flat fallbacks `permissions_mask`, `permissions_ceiling`) validated in Rust against parent capabilities (`requested <= parent`). Defaults to safe floor (`readonly`, `safe`).
  - *Hard Capability-Mask Block (No In-Flight Elevation):* A `readonly` sub-agent attempting a mutating tool (`capability_denied`) never requests in-flight elevation over mTLS. The engine halts actuation, unwinds recorded pre-mutation journal entries, and exits cleanly with `status: "permission_blocked"`.
  - *Orchestrator Loop Re-Delegation:* The orchestrator loop ingests `permission_blocked`, assesses context, and re-delegates with explicit permissions if authorized. Re-delegations are bounded by a per-`(agent, task)` circuit breaker.
  - *Preserved Should Gate:* The Session-10 Should Gate (`handle_escalation_request`: Protected Policy, anti-spoofed static tier floor, reversibility verification, extended `%assess-risk%`, stricter-only clamping) is strictly preserved for authority-ceiling (`authority_exceeded`) escalations.
  - *Decision B Refinement (As-Built):* Delegation calls remain ungated orchestration, but child mutating attempts under a `readonly` mask produce `permission_blocked` + orchestrator re-delegation rather than in-flight mTLS overrides.

### Threats (explicit)
- **Prompt injection** into the evaluator → mitigated by minimal context + stricter-only clamp + non-pardonable policy floor.
- **Forged control messages / unauthorized connection** → mutual-TLS WebSocket with ephemeral fingerprint-pinned keys, channel-bound challenge–response (leaked credentials non-replayable), loopback-only binding; connections failing the handshake are rejected before any message.
- **Cost/latency** → `Safe` fast-path + plan-time batch + flagged-only re-check; separate cheap evaluator model.

## 7. Session Resumption & Write-Ahead Log (WAL) Journaling

**Driver:** Long-running system diagnostic sessions must survive network dropouts, API rate-limits, or user interrupts (`SIGINT`) without re-running expensive diagnostic probes.

### Approach
1. **Append-only event log** — stream structured turn events (`TurnStart`, `ToolCall`, `ToolResult`, `Plan`) to `$XDG_RUNTIME_DIR/aichat-<session-id>.wal` (JSON-L).
2. **CLI resume flag** — `aichat --resume <session-id>` reconstructs conversation context + completed tool artifacts from the journal and continues from turn N.
3. **Session checkpointing** — atomic snapshot on clean loop completion or graceful cancellation.

## 8. Dynamic Multi-Turn Context Compaction

**Driver:** Extended 15+ turn troubleshooting investigations accumulate large message queues that consume tokens and degrade LLM reasoning.

### Approach
1. **Context-window threshold monitoring** — track accumulated prompt tokens per turn; when > a configurable ceiling (e.g. 70% of window), trigger rolling compaction.
2. **Rolling micro-summarization** — summarize turns 1…(N-3) into a dense structured system-state block (active hypothesis, verified facts, failed attempts) via a fast local/flash model.
3. **Recent-turn preservation** — retain the most recent 3 turns raw, resetting the token budget without losing immediate tool context.

## 9. Ephemeral Git Worktree Isolation for Multi-Agent Coding

**Driver:** When an orchestrator delegates to concurrent `coder` sub-agents inside a Git repository, children must build/edit/test without file clobbering or build collision.

### Approach
1. **Worktree provisioning** — `git worktree add --detach /tmp/aichat-wt-<pid> HEAD`.
2. **Isolated child CWD** — spawn `aichat --agent coder` with `Cwd = /tmp/aichat-wt-<pid>`.
3. **Diff return & consolidation** — child compiles/tests in its private worktree, returns a unified diff/patch.
4. **Cleanup** — parent sequentially reviews/applies patches, then `git worktree remove --force`.

> **Containment Spectrum:** #9 (micro-worktrees) is the L2 mid-point of the containment continuum (see View 1). The macro end (long-lived branches, human review) is delegated to the L3 supervisor `bohay`, not built into the engine.

## 10. Staged Configuration & Dry-Run Protocol for System Operations

**Driver:** Host config mutations (e.g. `/etc/caddy/Caddyfile`, K8s manifests) require pre-flight syntax validation and rollback safety before live activation.

### Approach
1. **Staging directory convention** — system mutation tools target `/tmp/staging/` rather than live host files.
2. **Validator tool integration** — pair staging tools with explicit checks (`nginx -t -c ...`, `caddy validate`, `kubectl diff`, `terraform plan`).
3. **Atomic apply & rollback** — orchestrator verifies pre-flight validation, takes an atomic backup (`.bak` / `etckeeper` snapshot), copies staged config to the live destination.

> **Containment Spectrum:** #10 (staging / dry-run) is the L1/L2 per-mutation end of the continuum, alongside #9 (micro-worktrees).

## 11. Mock-Client Test Seam for Agent-Loop Orchestration Coverage

**Driver:** #5 raised `agent_loop.rs` unit coverage 46.8%→64.7% by testing pure/near-pure helpers. The remaining ~35% is the `run()` orchestration loop — turn iteration, streaming, event emission, tripped-call short-circuit dispatch, nested sub-agent recursion — all gated behind a live `call_llm_raw` and only exercised by the billed, non-deterministic E2E harness.

### Approach
1. **Inject an LLM turn source** — refactor `run()` (or `call_llm_raw`) so "produce `(ChatCompletionsOutput, Vec<ToolCall>)` for this turn" is injectable (a `trait LlmTurnSource`, a boxed async closure, or a small `Client` mock).
2. **Deterministic scripted turns** — feed a fixed sequence (turn 1 → two tool calls; turn 2 → a `_plan`; turn 3 → final text) with no provider/network/cost.
3. **Cover the orchestration branches** — multi-turn iteration, turn-budget exhaustion end-to-end, `_plan` partition, tripped-call short-circuit, `CostExhausted` early return, sub-agent recursion.

**Notes:** deliberately Low — the decision logic is already unit-tested (#5) and end-to-end is covered by the harness; this buys *deterministic, offline* coverage of the wiring. The seam is a real hot-path production change → warrants its own spec and behavior-preserving review.

## 12. Remote MCP Transports (HTTP / WSS)

**Driver:** The native Rust MCP engine (#1) speaks only stdio to locally-spawned servers. The roadmap's Layer-4 control planes and remote MCP servers require **HTTP and WebSocket (WSS)** transports — the concrete L2 engine work that unblocks the roadmap's "Remote MCP" arrow.

### Approach
1. **Transport abstraction** — a server reachable via stdio (current), HTTP, or WSS, selected by its config entry (a `url:` / `transport:` field alongside `command:`).
2. **Auth pass-through** — bearer/token headers for remote servers (reuse the config-field env-expansion used for API keys).
3. **Reconnect & timeout semantics** — connection timeouts + reconnect distinct from the stdio spawn model; keep lazy-connect + per-server pooling.
4. **Transparent** — remote MCP tools MUST still appear as regular `FunctionDeclaration`s; no changes to `use_tools`, agents, or the loop.

**Notes:** extends Transparent MCP upward; enables L4 remote coordination without changing the tool model; preserves Pillar 6 (single static binary; stdio remains the zero-config default for bastion/air-gapped deployments).

## 13. Scoped Shared Artifact Store for Orchestration Trees

**Driver:** Sub-agents collaborate only through (a) the orchestrator's context, (b) tool output routing (#4), (c) read-only status files. There is no way for sibling agents in one tree to share intermediate artifacts (dedup an expensive fetch/embed; producer→consumer handoff) **without** round-tripping the parent. Engine-level counterpart to the L4 "Hive-Mind" — scoped to a single local tree.

### Approach (deliberately NOT a free-form blackboard)
1. **Scoped to one tree** — keyed by the root orchestrator PID (`AICHAT_AGENT_ROOT`), under `$XDG_RUNTIME_DIR/aichat-tree-<root-pid>/`. Sub-agents inherit the key via env, like `AICHAT_AGENT_DEPTH`.
2. **Structured, append-only, declared writes** — agents write via a declared tool (`artifact_put name=...`), not ambient shared memory; reads are `artifact_get` / `artifact_list`.
3. **Read-mostly for siblings** — parallel agents read freely (dedup/handoff) but the write surface is explicit and auditable.
4. **Lifecycle** — created on root-agent start, torn down on completion; stale trees cleaned at startup via `/proc/<pid>`.

**Notes:** Low priority and deliberately narrow — a general "agents chat freely" scratchpad fights the fork's process-isolation / context-hygiene thesis. Interacts with #7 (WAL) and #9 (worktrees). The L4 "Hive-Mind" (cloud, cross-fleet) remains a separate roadmap-only concept above this.

## 14. Per-Machine Consolidated Audit Log (Auditability as a Distinct Goal)

**Driver:** Today's observability (Pillar 4) is **live, ephemeral, single-process** (`/dev/tty`, OSC titles, per-PID status files that vanish). There is no **durable, consolidated, historical** record an external auditor/SRE/observability platform can use — long after every process exited — to reconstruct what aichat did on a host over time: which agents ran, in what tree, what tools at what tier, what verdicts/escalations, which mutations were actuated and which reverted, by whose authority. **Auditability is a distinct goal from observability.**

Three distinct planes (vs. the #6d design): **control+telemetry** (ephemeral WebSocket), **durability** (#6d rollback journal), **audit** (this item — durable, consolidated, read by non-participants possibly much later).

### Approach (sketch — needs its own spec)
1. **Each agent authors its own records** independently (Pillars 1/2). No central logging daemon at runtime.
2. **Consolidated per-machine view** via correlation IDs (`tree_id`, `agent_id`/`pid`, `parent_id`, `depth`, `timestamp`, per-agent `sequence`) — reconstruct the tree + timeline **at read time**, no runtime coupling.
3. **Format: JSON Lines**, append-only — ingestible by `jq`/`tail`/Loki/Vector/Splunk. Per-process files merged into a machine view, vs. one contended `O_APPEND` file (atomic <PIPE_BUF records).
4. **Records reference, never embed, payloads** — log *that* a tool ran + safety metadata (tier, verdict, artifact path/hash), NOT raw outputs (avoid context-monolith-on-disk + PII surface). Payloads live in #4 / #13.
5. **Rotation + retention** (size/age) — mandatory on small bastions.
6. **Optional hardening (later):** hash-chained records for tamper-evidence.

**Notes:** natural home to fix the `$0.000000` cost bug (structured `cost` records vs stderr scraping). Complementary to #7 (WAL is read by aichat to *resume*; audit is read by humans/platforms for *forensics*). Delivery: write JSONL + let an external shipper tail it (pull, zero coupling, Pillar 6 clean). #6 is a primary producer, but the capability stands alone.

---

## 15. Serious Structured `_plan` / Plan-Driven Execution (+ whole-plan risk pre-pass)

**Driver:** The current `_plan` pseudo-tool is a **free-text scratchpad** — a single `{thought: string}` the model writes to itself, which the loop logs, acknowledges (`"acknowledged"`), and drops into the next turn's context. It never *drives* execution: the loop is turn-by-turn ReAct, so the "plan" has no structural relationship to the tool calls that follow. This is thin, and it directly limited #6c: there was no structured plan to attach risk flags to (see #6c as-built notes — the literal "plan-time flag key steps" model was superseded by an act-time raise-only cache).

A **serious** planner would make the plan a first-class object that the loop executes against, tracks progress through, and replans on failure — and, as a direct payoff, would enable a **whole-plan risk pre-pass**.

### Approach (sketch — needs its own spec)
1. **Structured plan schema.** Evolve `_plan` from `{thought}` to structured steps, e.g. `{ rationale, steps: [{ id, tool, intent, args_preview, depends_on[] }] }` — a declared DAG/sequence of intended actions, not just prose.
2. **Plan-driven execution.** The loop tracks plan state (pending/done/failed/skipped per step), executes against it, and replans on deviation/failure — rather than treating each turn as unrelated. Must degrade gracefully: a model that ignores `_plan` still runs the plain ReAct loop (the plan is an optimization/structuring layer, never a correctness dependency).
3. **Whole-plan risk pre-pass (the #6c payoff).** With declared steps available, run a *plan-time* `%assess-risk%` pass that sees the **whole plan at once** — enabling cross-step reasoning a single-action act-time view structurally cannot have ("step 3 deletes what step 5 needs"; "these five steps together exfiltrate"). It **writes into the existing monotonic raise-only `RiskCache`** (`src/safety.rs`), pre-raising the authority floor of risky steps *before* the agent walks toward them.
4. **Strictly a red-light, never a green-light.** The pre-pass can only ever *pre-raise* a cached floor (early, cheaper stop, richer context). It can never pre-clear an action. The **act-time evaluation remains the non-negotiable floor** — every non-`Safe` action is still evaluated (or served from the raise-only cache) immediately before it runs. This is the invariant that made it safe to *widen* the plan-time context: more context can only help the evaluator say "no" harder, and a stricter-only clamp + raise-only cache neutralize any injected attempt to say "yes."

**Rationale / fit:** improves planning quality generally (a real capability, not just a safety feature), and completes #6c's two-phase intent faithfully without ever weakening the deterministic floor. Changes the agent-loop contract (plan state, replanning), so it is deliberately **its own item** rather than smuggled into a safety increment.

**Dependencies:** consumes #6c's `RiskCache` and `%assess-risk%` evaluator. Interacts with #7 (WAL — plan state is checkpointable) and #10 (staged/dry-run ops). Priority **Medium**; own spec required; touches the hot path.

### As-Built & Verification (Session 26)
- **Structured Plan Schema & Resilient Dual-Arm Parser (`src/agent_loop/plan.rs`)**: Formal `_plan` JSON Schema featuring `objective`, typed `steps` (`id`, `intent`, `tool`, `args_preview`, `depends_on`), and `thought`. Resilient `PlanPayload` deserializes structured payloads while gracefully falling back to legacy strings or partial object structures without erroring or discarding intent.
- **Plan State Tracking & Live Progress Events**: Added `PlanTracker` tracking step states (`Pending`, `InProgress`, `Completed`, `Failed`, `Skipped`). Emits `PlanReceived` and `PlanStepUpdated` events across the agent loop.
- **Ahead-of-Time Risk Pre-Pass (`plan_risk_prepass`)**: Evaluates planned tools in advance against `#6c`'s `%assess-risk%` evaluator, storing pre-raised ceilings in the monotonic, raise-only `RiskCache`. Emits `PlanRiskPrepassFlagged` on risky steps. Degrades cleanly to a no-op when `#6c` is absent (`test_prepass_noop_without_6c`).
- **Verification**: All 128 tests in `agent_loop::tests` pass; all 529 workspace tests pass.
- **Spec:** [`.kiro/specs/structured-plan-and-skills/spec-15-structured-plan.md`](../specs/structured-plan-and-skills/spec-15-structured-plan.md)

---

## 16. Web-Search Style & Branch-Wide Grounding Control (`--wslinks`)

**Driver:** Web research in orchestrator sub-agent swarms historically enforced a rigid multi-turn pipeline: `web_search` with `--links` to discover URLs, followed by 2–4 sequential or parallel `fetch_and_summarize` calls (each shelling out to `html-to-markdown` and piping through `summarize_text`). For general comparison, inquiry, or high-level research tasks, the search provider (e.g. Gemini with Google Search grounding) already provides an authoritative, up-to-date grounded answer with citations directly in the first turn. Running 3–5 additional page scrapes burns tokens, consumes 30–60 seconds, and risks 404/bot-block failures.

### Approach
1. **Branch-wide `--wslinks` CLI flag (`src/cli.rs`)**: Add `--wslinks` flag. When present, export `AICHAT_WSLINKS=true` into the process environment.
2. **Subprocess propagation (`src/agent_loop.rs`)**: `eval_agent_tool_subprocess` passes `--wslinks` and `AICHAT_WSLINKS=true` to all child subagents across the delegation hierarchy.
3. **Dynamic instruction variable `{{__researcher_search_instructions__}}` (`src/config/agent.rs` / `src/utils/variables.rs`)**:
   - Default (absence of `--wslinks`): directs `researcher` to use direct grounded `web_search` (`links: false`) and synthesize immediately without fetching web pages.
   - Active (`--wslinks`): directs `researcher` to discover links (`links: true`) and fetch 2–4 pages with `fetch_and_summarize`.
4. **Tool Guard (`tools/web_search_aichat.sh`)**: Enforce that `--links` formatting is only activated when `AICHAT_WSLINKS=true`.
5. **Trace display optimization**: Omit the redundant initial `Agent <name> loop trace:` header lines, letting clean turn entries (`[turn 1/20] starting`) and guide rails speak for themselves.
6. **Dual Demo Verification**: Demo 5 tests `--wslinks` (multi-step scraping pipeline); Demo 5 default tests direct grounded search (single-turn per researcher).

### As-Built & Verification (Session 22)
- **Dual-Layer `MALFORMED_FUNCTION_CALL` Recovery**: Diagnosed and resolved Gemini 2.5 Flash parallel burst hallucinations emitting Python function syntax (e.g. `print(default_api.web_search(...))`) instead of JSON function calls. Added system prompt guidance, AST/kwargs parser in `src/client/vertexai.rs` (`recover_malformed_function_call`), and transient backoff retries in `src/agent_loop.rs` (`call_llm_raw`).
- **Truthful Tool Failure Reporting**: Fixed `eval_tool_calls_parallel` to inspect `value.get("error").is_some()`, rendering sub-agent non-zero exits as `FAILED` in red instead of masking as `completed`.
- **URL Token Formatting**: Formatted canonical URLs on dedicated lines (`Title: ...\nURL: ...\nSummary: ...`) in `tools/web_search_aichat.sh` in `llm-functions`, eliminating markdown link bracket corruption of base64 redirect tokens.
- **Verification**: Isolated parallel researcher test passed with exit code 0; live Demo 5 (`--wslinks`) passed in 40.6s with 0 crashes and 0 retries; live Demo 5 (direct grounded) passed in 33.9s; 513/513 workspace tests passed; 71/71 `argc test` passed.

### As-Built & Verification (Session 23)
- **Branch-Exclusive Agent Colors**: Implemented zero-overhead hierarchical palette stratification in `src/agent_loop.rs` (`allocate_subagent_color`). Orchestrator defaults to index 0 (`cyan`); child sub-agents cycle through remaining 10 high-contrast colors via an atomic sequence counter in `eval_agent_tool_subprocess` exported via `AICHAT_AGENT_COLOR` and `AICHAT_SUBAGENT_SEQ`.
- **Nano Caller Color Inheritance**: Preserved and verified caller color inheritance in `src/function.rs` (`current_agent_color_name(&invoking_agent)`). Nano-tools (`# @meta nano true`) inherit the exact color of their invoking agent.
- **Verification**: Unit tests `test_allocate_subagent_color_exclusivity` and `test_nano_worker_inherits_caller_env_color` pass; all 117 tests in `agent_loop::tests` pass; all 507 unit/integration tests pass.

**Spec:** [`.kiro/specs/web-search-style-control/`](../specs/web-search-style-control/)

---

## 17. Progressive Disclosure Runbooks & Skills (`read_skill`)

**Driver:** Standard agent architectures inject extensive procedural runbooks, troubleshooting guides, and API schemas directly into the initial system prompt, causing severe context-window bloat, token inefficiency, and confusion when multiple runbooks conflict. Progressive disclosure patterns (like Anthropic Skills and Antigravity) instead present a compact metadata catalog (`name`, `description`) in the system prompt, allowing the agent to dynamically inspect detailed instructions on demand via a tool (`read_skill`). However, untrusted workspace runbooks present a critical security threat: malicious repository runbooks could attempt to prompt-inject or "pardon" destructive operations.

### Approach
1. **Unified Skill Discovery (`src/skill.rs`)**: Scans 3 discovery tiers with strict precedence: Workspace (`.kiro/skills`, `.agents/skills`, `.skills`) > Global (`~/.config/aichat/skills`) > Builtin. Workspace skills are explicitly tagged `SkillProvenance::WorkspaceTainted`.
2. **Plain YAML Frontmatter Parser**: Parses standard `SKILL.md` frontmatter (`name`, `description`) without requiring mandatory cryptographic signatures or hashes, ensuring developer ergonomics.
3. **Agent Eligibility Gating (`src/config/agent.rs`)**: Agent configs configure `skills: all | false | [skill1, skill2]`. Nano utility workers (`@meta nano true` / `nano: true` / `AICHAT_AGENT_NANO=true`) are completely excluded from skill discovery and tool injection.
4. **Dynamic Tool Injection & Execution (`src/config/mod.rs` & `src/agent_loop.rs`)**: When eligible skills exist, `read_skill` is injected and handled via Route 0 in `eval_single_tool`, returning JSON containing `name`, `description`, `instructions`, `provenance`, and `path`.
5. **Step-Scoped Active Taint Lifecycle (`ActiveSkillTracker`)**: Loading a workspace skill activates taint (`untrusted_runbook: true` and `active_tainted_skills`). Taint is tracked per plan step and cleared when the step completes (`complete_step`) or when consumed.
6. **Evaluator Integration & Prepass Simulation (`src/safety.rs` & `src/agent_loop.rs`)**: Added heightened scrutiny Directive 5 in `%assess-risk%`. Planned `read_skill` steps simulate taint in `plan_risk_prepass` so downstream steps evaluate under heightened scrutiny in advance.

### As-Built & Verification (Session 26)
- **Core Implementation**: Implemented `src/skill.rs`, updated `src/config/agent.rs`, `src/config/mod.rs`, `src/safety.rs`, `src/agent_loop.rs`, and `assets/roles/%assess-risk%.md`.
- **Verification**: All 6 unit tests in `src/skill.rs` pass; all 128 tests in `agent_loop::tests` pass; all 64 tests in `safety::tests` pass; full test suite (529 pass, 0 fail); clippy clean (`-D warnings`).
- **Spec:** [`.kiro/specs/structured-plan-and-skills/spec-17-skills.md`](../specs/structured-plan-and-skills/spec-17-skills.md)

---

## Items #18–#25: Distilled from TrueForge & KubeIntellect (Session 27)

> These items were distilled from studying [`truefoundry/trueforge`](https://github.com/truefoundry/trueforge) and [`MSKazemi/kubeintellect`](https://github.com/MSKazemi/kubeintellect). Full analysis: [`sre-and-supervisory-landscape.md`](sre-and-supervisory-landscape.md). Detailed ranking & justification: [perry-enhancements-from-trueforge-kubeintellect.md](file:///home/istari/.gemini/antigravity-cli/brain/6469c488-bfa7-45d4-b062-3ced2df64dd6/perry-enhancements-from-trueforge-kubeintellect.md).

## 18. Dedicated SRE Telemetry Actuators & Multi-Agent Triage (`host_env`, `host_service`, `host_resource`, `host_net`, `host_logs`, `sre` Agent) ✓

**Driver:** `innators` previously relied on generic `execute_command.sh` for infrastructure operations, emitting unbounded, hallucination-prone shell strings. KubeIntellect's 4-pillar triage model (Pod/Metrics/Logs/Events) demonstrated the value of structured, schema-bounded infrastructure tools. We translated this model from K8s to **Linux Hosts, Bare-Metal, VMs, and Containers**, engineering it to withstand the test of flavor (Debian, RHEL, Alpine, Arch) and the test of time (2014–2026+), anchored by a dedicated baseline identity actuator and executed via multi-agent parallel orchestration.

**Predecessor:** 🆕 Entirely new. Implemented across companion repository [`innators`](file:///home/istari/projects/innators) and [`perry`](file:///home/istari/projects/perry). Plugs into Perry's **#6** (tool safety modes) and executes autonomously under `--autonomy readonly` (A0) as `# @meta risk safe`.

### As-Built Implementation (Session 30)
1. **`host_env.sh` (Pillar 0 — Baseline Identity & Environment):**
   * Actions: `summary`, `hardware`, `os`, `virtualization`.
   * Establishes the canonical hardware and OS anchor: hostname, architecture, CPU topology (cores, model, MHz), physical RAM, cgroups memory limits, primary root block device/mount, and hypervisor/container boundary.
   * 10–15+ year portability: Pure integer `/proc/uptime` (works on minimal Alpine/busybox where `uptime -p` crashes), 4-tier CPU model fallback (`model name` $\to$ `Model` $\to$ `Hardware` $\to$ `lscpu`), and multi-tier OS release parsing.
2. **`host_service.sh` (Pillar 1 — Services & Processes):**
   * Actions: `failed`, `status`, `top_procs`, `cgroup_limits`.
   * Cross-init compatibility: Auto-detects systemd vs. SysVinit/Upstart fallback (`service --status-all`). Handles modern bullet dots (`●`) and legacy column layouts.
   * Container/Pod awareness: Detects cgroups v1 (`memory.limit_in_bytes`) and cgroups v2 (`memory.max`, `cpu.stat`) limits.
3. **`host_resource.sh` (Pillar 2 — USE Metrics):**
   * Actions: `summary`, `cpu`, `memory`, `storage`.
   * Sourced from 10-year kernel invariants: `/proc/stat`, `/proc/meminfo`, `/proc/loadavg`, `/proc/pressure/memory` (PSI), POSIX `df -mP` and `df -iP`, `/proc/mounts` (read-only mount detection), and `/proc/diskstats`.
4. **`host_net.sh` (Pillar 3 — Network & Transport Telemetry):**
   * Actions: `interfaces`, `sockets`, `listen_queues`, `conntrack`.
   * Bypasses post-2016 `ip -j` failures by reading `/proc/net/dev` directly. Audits saturated TCP listen queue backlogs (`ss -lnt` `Recv-Q > Send-Q`) and conntrack table saturation.
5. **`host_logs.sh` (Pillar 4 — Logs & Fault Events):**
   * Actions: `recent_errors`, `timeline`, `kernel_faults`, `security_denials`, `crashes`.
   * Standardizes on `journalctl -p err -n 25 -o json` with legacy syslog fallback (`/var/log/messages`, `/var/log/syslog`). Materializes `/tmp/perry-host_logs-query-<pid>.sh` and dump artifacts.
   * Security module auditing: Automatically probes SELinux denials (`ausearch` / `audit.log`) on RHEL and AppArmor denials (`dmesg`) on Ubuntu/Debian.
   * Transparent Decoupled Distillation: Perry intercepts heavy JSON outputs and invokes `%distill-telemetry%` via the evaluator model endpoint.
6. **`sre` Specialist Subagent (`innators/agents/sre/`):**
   * Dedicated persona equipped with all 5 host telemetry actuators. Registered in `agents.txt` and callable by `orchestrator`.
7. **Multi-Agent Parallel Orchestration (Demo 25) & SRE Incident Demos (Demos 26, 27):**
   * **Demo 25:** Orchestrator plans with `_plan` and delegates the 5 pillars in parallel to 5 concurrent `sre` subagents, finishing in 4 turns (zero turn exhaustion).
   * **Demo 26:** SRE agent correlates failed `thermald.service` with logs and resource pressure anchored to `host_env`.
   * **Demo 27:** SRE agent executes 24h historical telemetry and transparent decoupled distillation.
8. **Output Symmetry via `jq` & Semantic Distillation:**
   * All actuators pipe through `jq` to emit clean, typed JSON records. Telemetry tap performs dynamic semantic key-value extraction without rigid schema enums.

---

## 19. Autonomy Ladder (`--autonomy <readonly|consult|reversible>`) ✓

**Driver:** Perry already has a rich 2D safety foundation (graduated `BlastRadius` tiers, orthogonal proven reversibility, and capability masks in #6; persistent mTLS escalation in #6d). The Autonomy Ladder provides domain-agnostic, operator-facing macro presets to coordinate both safety axes without environment variable proliferation or conflation.

**Predecessor:** 🔧 Enhancement of **#6** (tool safety modes) and **#6d** (escalation & HITL). Session-level macro coordinating existing safety mechanisms.

### As-Built Implementation (Session 28)
1. **`--autonomy readonly` (Observer / A0):** Root macro expands capability mask to `readonly` and baseline ceiling to `safe`. Mutating tools are rejected immediately at Gate 1 with `capability_denied`—spending 0 evaluator tokens and triggering zero human prompts.
2. **`--autonomy consult` (Copilot / A1):** Unmasked capability with a `safe` authority ceiling and **clamped autonomous Option B reversibility**. Any mutating tool trips Gate 2 and routes through the **Evaluator-First Unified Human Consultation Funnel**: `%assess-risk%` audits context and arguments first, presenting a single combined prompt to the operator.
3. **`--autonomy reversible` (Safe Autonomous / A2):** Unmasked capability with a `reversible` ceiling and active Option B preflight remediation. Tools declaring `# @meta reversible-via backup` atomically record backups in the 0600 rollback journal and execute autonomously; only irreversible mutations require human consultation.
4. **Anti-Soup Sandboxing:** `--autonomy` is strictly an orchestrator-level CLI/config macro. Child sub-agents never receive an autonomy environment variable; they inherit canonical, statically provisioned `DelegatedPermissions` (`AICHAT_CAPABILITY_MASK` and `AICHAT_AUTHORITY_CEILING`) and cannot escalate past their parent.
5. **Verification:** All 558 unit/integration tests passing; clippy clean (`-D warnings`); live Demo 24 (all 3 postures) and migrated Demos 8, 10b, and 18 passing.

## 20. Zero-Token Pre-Flight Triage (`--preflight`)

**Driver:** KubeIntellect's compiled playbook predicates (`kq findings`) evaluate cluster state without any LLM call. Perry should not burn a $0.02–$0.10 LLM reasoning loop when a deterministic bash check can resolve the question in <50ms at $0.00.

**Predecessor:** 🆕 Entirely new. #17 (Runbooks) discloses instructions *to* the LLM; preflight bypasses the LLM entirely.

### Approach
1. **CLI flag:** `--preflight <command>` — executes the command as a subprocess before initializing the agent loop.
2. **Short-circuit on success:** If exit code = 0, print captured stdout and terminate immediately. No LLM API call, no token cost.
3. **Grounded context on failure:** If exit code ≠ 0, capture stdout + stderr as structured telemetry and inject as grounded initial context into the normal agent loop.
4. **Example:** `aichat --preflight "curl -sf http://localhost:9090/-/healthy" --agent sre "Why is Prometheus unhealthy?"`

**Notes:** ~80-120 lines Rust. The short-circuit path is before `run()` — no interaction with the agent loop, safety pipeline, or tool dispatch.

## 21. In-Loop Clarification Tool (`ask_user`)

**Driver:** TrueForge implements human checkpoints for ask-user-questions and Generative UI. During live incident triage, agents encounter ambiguous states (pod name in multiple namespaces, unclear scope for destructive operations) and either halt with text or guess — both dangerous in production.

**Predecessor:** 🔧 Enhancement of **#6d** (escalation & HITL). Extends the existing `/dev/tty` / mTLS escalation channel from safety-only to general clarification.

### Approach
1. **Built-in tool:** `ask_user` registered as a Route 0 built-in (like `read_skill` in #17).
2. **Prompt to `/dev/tty`:** Write the structured question to `/dev/tty` (visible even in pipes, per Pillar 4).
3. **Read response:** Read a line from `/dev/tty` (interactive) or from the #6d escalation transport (headless/Layer 3 supervisor).
4. **Return as tool output:** The operator's response becomes the tool result, feeding back into the LLM context for the next turn.

**Notes:** ~30 lines of Rust. Zero dependencies on unshipped backlog items — leverages existing `/dev/tty` infrastructure and the #6d `EscalationTransport` trait. Can land immediately.

## 22. Deferred Tool Loading (Lazy Schema Injection)

**Driver:** TrueForge implements deferred tool loading as part of its context engineering. Perry currently injects all tool JSON schemas (~2,000–4,000 tokens) into the system prompt on every turn, even when most tools go unused in a session.

**Predecessor:** 🔧 Enhancement of **#8** (context compaction) and **#17** (progressive disclosure runbooks). Applies the progressive disclosure principle from #17 (runbook schemas loaded on demand) to tool schemas.

### Approach
1. **Agent config schema extension:** Declare tool groups with a `load: deferred | immediate` flag.
2. **Lightweight catalog injection:** For deferred groups, inject only tool names and one-line descriptions into the system prompt.
3. **`list_tools(group)` meta-tool:** When the model signals intent to use a tool domain, it calls `list_tools("sre")` which injects the full schemas for that group into subsequent turns.
4. **Graceful degradation:** If the model calls a deferred tool directly without calling `list_tools` first, the engine auto-loads the schema and proceeds.

**Dependencies:** none strictly, but enhanced by #8 (compaction) and #17 (skill discovery infrastructure).

## 23. Flight Recorder & Session Replay (`--replay`)

**Driver:** KubeIntellect's flight recorder (`kq replay`) provides hash-chained, tamper-evident decision logs for incident post-mortems and compliance auditing. Perry's existing observability (status files, `/dev/tty` traces) is live/ephemeral — no durable, integrity-verified historical record.

**Predecessor:** 🔧 Enhancement of **#7** (WAL journaling) and **#14** (audit log). Adds hash-chaining and a replay CLI on top of their event stream infrastructure.

### Approach
1. **SHA-256 hash chain:** Each JSONL event line (from #7 WAL / #14 audit log) includes a `prev_hash` field containing the SHA-256 hash of the previous line. Provides tamper-evidence on minimal bastions without requiring external PKI.
2. **`--replay` CLI subcommand:** `aichat --replay session.wal` steps through past sessions, rendering each turn's prompt, tool calls, outputs, operator approvals, and cost — color-coded and formatted for terminal post-mortems.
3. **Integrity verification:** `--replay --verify` validates the full hash chain and reports any broken links.

**Dependencies:** **Blocked on #7 (WAL) and #14 (Audit Log).** The hash chain itself is ~15 lines of Rust; the replay CLI is ~100 lines. The value is in landing *after* the event stream exists.

## 24. Read-Only Batch Actuator Execution (`run_batch`)

**Driver:** TrueForge's Code Mode allows batch tool execution in a single turn. During SRE discovery phases, agents waste 5–8 sequential turns running simple read commands (`df -h`, `uptime`, `kubectl get nodes`, `promql up{}`).

**Predecessor:** ⚡ Overlaps **#3** (agent loop — parallel Tokio dispatch) and **#15** (plan-driven execution — structured multi-step execution with risk pre-pass). `run_batch` is a lightweight, read-only-gated subset of #15's plan executor.

### Approach
1. **`run_batch` meta-tool:** Accepts a JSON array of tool invocations `[{tool, args}, ...]`.
2. **Parallel dispatch:** Uses existing #3 Tokio parallel tool dispatch infrastructure.
3. **Aggregated return:** All results returned as a single structured tool output.
4. **⚠️ Hard-gated to `read-only` tools only.** Any batch containing a `reversible` or `dangerous` tool is rejected at the engine level, forcing the model to issue those calls individually through the normal #6 safety pipeline. This is non-negotiable — batch execution bypasses per-tool safety gates, and Perry's safety model evaluates each invocation individually.
5. **Alternative path:** If #15's plan executor lands first, `run_batch` may be unnecessary — the plan executor with a whole-plan risk pre-pass can batch execution naturally while preserving full safety screening.

**Notes:** Priority is Low precisely because of the #15 overlap. If #15 ships, this may be folded into it.

## 25. Lightweight Incident Episode Store

**Driver:** KubeIntellect's temporal memory hierarchy preserves past incident episodes and bi-temporal knowledge graphs. When a recurring incident occurs, the engine retrieves prior resolutions for similar symptoms.

**Predecessor:** 🔧 Enhancement of **#13** (scoped shared artifact store). Extends #13's root-PID-scoped, single-tree, read-mostly artifacts to cross-session persistence with write paths, deduplication, and TTL.

### Approach
1. **SQLite episode store:** New `src/episodes.rs` module with a lightweight SQLite database at `$XDG_DATA_HOME/aichat/episodes.db`.
2. **Write path:** On session completion (or explicit `save_episode` tool call), persist a structured summary: symptoms, telemetry queries used, root cause, resolution steps, timestamp.
3. **Read path:** On session start, query the episode store for symptom-similar past incidents and inject as high-relevance context.
4. **TTL & eviction:** Configurable maximum episode count and age to prevent unbounded disk growth on 64MB bastions.

**Notes:** This is a **new subsystem, not a RAG extension.** Perry's hybrid RAG (`src/rag/`) is retrieval over static documents. Incident episodes require write paths, dedup, schema management, and TTL — fundamentally different from HNSW + BM25 over markdown files. Priority is Low; depends on #7 (WAL) for the writer infrastructure.

---

## Dependency Graph (items #1–#4)

The foundational tool-execution items are coupled:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Shared Tool Execution Layer                    │
│         (eval_tool_calls, run_llm_function, ToolResult)          │
└──────────────────────────────┬──────────────────────────────────┘
                               │
          ┌────────────────────┼────────────────────────┐
          │                    │                         │
  ┌───────▼────────┐  ┌───────▼─────────┐  ┌──────────▼──────────┐
  │ #1 Native MCP  │  │ #3 Client-Side  │  │ #2 Gemini           │
  │ (tool exec)    │  │ Loop Enhance    │  │ Interactions        │
  └───────┬────────┘  └───────┬─────────┘  └──────────┬──────────┘
          │              depends on #1                  │
          └────────────────────┼────────────────────────┘
                               │
                    ┌──────────▼──────────┐
                    │  All feed the same   │
                    │  FunctionDecl +      │
                    │  ToolResult model    │
                    └─────────────────────┘
```

Sequencing rationale: **#1** is pure infrastructure, no architectural risk. **#3** is the highest-leverage daily improvement (makes *every* provider better) and needs #1 so MCP tools participate in parallel execution + sub-agent delegation. **#4** builds on #3 (routing in the async dispatch layer). **#2** is the longest-term bet (Google's API may shift); by then the tool layer is mature and the classic loop covers Gemini via `generateContent` + client-side orchestration.

## Future Considerations (not yet scoped, no item)

- **OpenAPI ingestion** — parse OpenAPI specs to auto-generate tool definitions, invoke via HTTP. A third backend for ToolDispatch.
- **Structured output abstraction** — uniform JSON Schema enforcement across providers (all support it, but aichat doesn't abstract it).
- **Context/memory management** — sliding window, summarization, or vector-backed recall for long sessions (partially addressed by #8).
- **Human-in-the-loop** — configurable pause points (now largely #6d).
