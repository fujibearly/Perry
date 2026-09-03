# aichat Fork — Strategic Roadmap

**Purpose:** a single consolidated view of the fork's strategic direction, replacing the fragments previously scattered across [`sre-and-supervisory-landscape.md`](sre-and-supervisory-landscape.md), the Session 1 summary, and the Session-2 architecture blueprint ([`architecture-blueprint-2026-08-28-to-2026-09-02.md`](architecture-blueprint-2026-08-28-to-2026-09-02.md) — the origin of the 4-Layer Taxonomy and the #5-#10 specs). It maps each strategic strand to the **4-layer architectural taxonomy** and to its **tracked backlog item** (or explicitly notes when none exists yet).

> **Roadmap vs. Backlog.** The *roadmap* (this doc) is strategic direction — mostly cross-cutting and Layer 3/4. The *backlog* ([`backlog.md`](backlog.md) / the table in [`progress.md`](progress.md)) is the tracked, actionable Layer-2 engine work queue. A roadmap strand is only "committed work" once it has a backlog item. Strands without one are aspirations, not obligations.

---

## The 4-Layer Taxonomy (orientation)

| Layer | What | Examples | Where the work lives |
|-------|------|----------|----------------------|
| **L1 — Actuation** | Deterministic CLI tools | `llm-functions` (31 Bash/`argc` tools) | Separate repo; some backlog items touch it (#6, #10) |
| **L2 — Execution Engine** | The reasoning loop, MCP, RAG, routing, budgets | **this `aichat` fork** | **The backlog is almost entirely here** |
| **L3 — Workspace Supervisors** | Terminal multiplexing, HITL gates, worktrees | `dot-agent-deck`, `bohay`, `AoE` | External tools (adoption, not our code) |
| **L4 — Enterprise Control Plane** | Multi-tenant auth, cloud audit, fleet coordination, shared cache | `TrueForge`, `Portkey`, **Fleet Commander / "Hive-Mind"** | **Proposed only — not built, mostly out of engine scope** |

Design intent: the engine (L2) is the fixed constant; L3/L4 sit *around* it and drive it via CLI/PTY (L3) or Remote MCP/HTTP/WSS (L4). This is why the roadmap and backlog barely overlap — the roadmap largely describes what surrounds the engine.

---

## Roadmap ↔ Backlog Crosswalk

| Strategic strand | Layer | Status | Backlog item | Notes |
|------------------|-------|--------|--------------|-------|
| Native in-process MCP | L2 | ✅ Shipped | **#1** (Done) | Replaced the Node.js bridge. |
| Provider-agnostic agent loop | L2 | ✅ Shipped | **#3** (Done) | Parallelism, budgets, sub-agents, `_plan`. |
| Declarative stream routing | L2 | ✅ Shipped | **#4** (Done) | Auto-cap, pipes, file targets. |
| Deterministic test/coverage hardening | L2 | ✅ Shipped | **#5** (Done) | agent_loop.rs 46.8%→64.7% line. |
| Tool safety modes / actuation governance | L1/L2 | 🔜 In progress | **#6** (High; umbrella #6a–#6d) | #6a (capability mask) implemented on `feat/tool-safety-6a`. #6b–#6d proposed. Escalation uses an mTLS **WebSocket** inter-agent channel (child dials parent) — HITL-adjacent, complements L3 approval gates, and forward-compatible with remote agents. |
| Session resumption / WAL | L2 | 🔜 Proposed | **#7** (High) | Durable state; survives dropouts/SIGINT. |
| Context compaction | L2 | 🔜 Proposed | **#8** (Med) | Fallback to the delegation-first hygiene model. |
| Ephemeral Git worktree isolation | L2 | 🔜 Proposed | **#9** (Med) | Engine-side complement to L3 `bohay`. |
| Staged config / dry-run ops | L1 | 🔜 Proposed | **#10** (Med) | SRE actuation safety; mostly `llm-functions`. |
| Mock-client test seam | L2 | 🔜 Proposed | **#11** (Low) | Deterministic coverage of `run()` orchestration. |
| **Remote MCP transports (HTTP/WSS)** | L2→L4 | 🔜 Proposed | **#12** (Med) | The concrete engine work that lets L4 control planes drive the engine remotely. |
| **Scoped shared artifact store** (engine-level cross-agent memory) | L2 | 🔜 Proposed | **#13** (Low) | Structured, root-PID-scoped, read-mostly. The *engine-level* counterpart to the L4 Hive-Mind — NOT a free-form blackboard. |
| **Per-machine consolidated audit log** (auditability, not just observability) | L2→L4 | 🔜 Proposed | **#14** (Med) | Durable append-only JSONL per agent, consolidated per-machine via correlation IDs; read by external auditors/observability platforms (L4-adjacent). Distinct *audit plane* from #6d's control + rollback planes. Surfaced during #6 design. |
| Gemini Interactions API | L2 | ⏸ Deferred | **#2** (Low) | Covered via OpenRouter + client loop. |
| CLI flag parity (`--show-trace/--max-turns/--max-cost`) | L2 | ✅ Largely shipped | *(folded into #3)* | Flags exist; no distinct open item. |
| **Adopt `dot-agent-deck`** (SRE mission control, HITL cards) | L3 | 🧭 External adoption | *(none — not our code)* | #1 SRE supervisor recommendation. Engine already emits the status-file/`/dev/tty` signals it consumes. |
| **Adopt `bohay`/Luvus** (worktree multiplexer, file leases) | L3 | 🧭 External adoption | *(overlaps #9)* | #1 code-refactoring supervisor. |
| **Agent of Empires (AoE)** (tmux fleet dashboard) | L3 | 🧭 External option | *(none)* | Host-dependent; lower portability. |
| **Fleet Commander MCP Server / "Hive-Mind"** (cloud shared semantic cache, fleet coordination) | L4 | 💭 Proposed concept only | *(none — see #13 for the engine-level slice)* | **Not built, not enumerated as phases, not in the backlog.** A cloud/fleet aspiration above the engine. |
| **TrueForge / enterprise K8s gateway** (RBAC, cloud audit) | L4 | 💭 External / concept | *(none)* | Also `Portkey` — LLM gateway/observability. Out of engine scope entirely. |

Legend: ✅ shipped · 🔜 proposed & tracked in backlog · ⏸ deferred · 🧭 external tool to adopt · 💭 concept only (no backlog item).

---

## Honest Status Notes

- **The "Hive-Mind" / 3-Tier Evolving Semantic RAG Cache is a concept, not code.** It appears only in the Session 1 summary (as a "formulated" idea) and in the SRE-landscape doc (as a *proposed* Layer-4 component). It is **not implemented and not in the backlog.** The only thing that exists today is the per-instance Layer-2 hybrid RAG (`src/rag/`, HNSW + BM25 + RRF) — retrieval, not shared cross-agent memory. Backlog **#13** captures the *engine-level, single-tree* slice of the idea; the cloud/fleet version remains an L4 aspiration.
- **The "six-capability" roadmap = backlog #5-#10, not a distributed-fleet phasing.** The Session-2 architecture blueprint ([`architecture-blueprint-2026-08-28-to-2026-09-02.md`](architecture-blueprint-2026-08-28-to-2026-09-02.md) §5) crystallized **six high-leverage Layer-2 engine capabilities** — these are exactly backlog items **#5, #6, #7, #8, #9, #10**. Earlier session-summary phrasing ("6-phase Distributed Fleet") was imprecise: there is no separate enumerated fleet-phasing plan; the "six" are the engine backlog items, all tracked. The Fleet Commander itself (below) remains a distinct, un-phased L4 concept.
- **L3/L4 tools are adoption recommendations, not fork deliverables.** `dot-agent-deck`, `bohay`, `AoE`, `TrueForge` are external projects. The engine's job is to emit the right contracts (status files, `/dev/tty` signals, Remote MCP via #12) so they can drive it — not to build them.

---

## The Containment Spectrum (from the Session-2 blueprint)

Containment in this system-level engine is not one mechanism but a spectrum across layers (blueprint §4). Three distinct patterns, each mapped to its backlog item:

| Pattern | Layer | Lifetime | Mechanism | Backlog |
|---------|-------|----------|-----------|---------|
| **System staging / dry-run** | L1/L2 | per-mutation | write to `/tmp/staging/`, run validators (`nginx -t`, `kubectl diff`), atomic apply + `.bak` rollback | **#10** |
| **Micro-worktrees** | L2 | seconds–minutes | ephemeral detached `git worktree add /tmp/aichat-wt-<pid>`, automated diff → orchestrator consolidation | **#9** |
| **Macro-worktrees** | L3 | hours–days | long-lived developer branches, human review/merge (external: `bohay`) | *(L3 tool)* |
| **Parallel read-only swarms** | L2 | per-turn | collision-free concurrent diagnostics (already shipped) | *(part of #3)* |

The takeaway: #9 (micro) and #10 (staging) are two points on the same containment continuum, and the macro end is deliberately delegated to an L3 supervisor rather than built into the engine.

---

## Suggested Sequencing (engine work only)

The backlog's own dependency notes still govern ordering. At a strategic level:

1. **High-priority safety/durability next:** #6 (Tool Safety Modes) and #7 (WAL Resumption) — both High, both core to the SRE stress-test. **#6a (capability mask) is done**; continue with #6b (tiers + policy + ceiling) → #6c (LLM risk evaluator) → #6d (mTLS-WebSocket escalation channel + human-in-the-loop).
2. **#12 Remote MCP** when L4 integration becomes concrete (it's the bridge that makes any L4 control plane useful). Note: #6d's WebSocket channel and #12 share the same WSS/transport muscle — the remote-agent generalization of #6d rides on it.
3. **#13 shared artifact store**, **#11 mock-client seam**, and **#14 audit log** are opportunistic — pick up when a dependent feature makes them cheap. #14 shares an append-only-JSONL writer with #7 (WAL) and is the natural home to fix the `$0.000000` cost-estimator bug.
4. **#2 Gemini Interactions** stays deferred unless OpenRouter coverage proves insufficient.

Pillar/Tenet alignment for each item is in [`progress.md`](progress.md) (the backlog table) and detailed in [`backlog.md`](backlog.md).
