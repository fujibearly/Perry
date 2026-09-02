# aichat Fork — Strategic Roadmap

**Purpose:** a single consolidated view of the fork's strategic direction, replacing the fragments previously scattered across [`sre-and-supervisory-landscape.md`](sre-and-supervisory-landscape.md) and the Session 1 summary. It maps each strategic strand to the **4-layer architectural taxonomy** and to its **tracked backlog item** (or explicitly notes when none exists yet).

> **Roadmap vs. Backlog.** The *roadmap* (this doc) is strategic direction — mostly cross-cutting and Layer 3/4. The *backlog* ([`backlog.md`](backlog.md) / the table in [`progress.md`](progress.md)) is the tracked, actionable Layer-2 engine work queue. A roadmap strand is only "committed work" once it has a backlog item. Strands without one are aspirations, not obligations.

---

## The 4-Layer Taxonomy (orientation)

| Layer | What | Examples | Where the work lives |
|-------|------|----------|----------------------|
| **L1 — Actuation** | Deterministic CLI tools | `llm-functions` (31 Bash/`argc` tools) | Separate repo; some backlog items touch it (#6, #10) |
| **L2 — Execution Engine** | The reasoning loop, MCP, RAG, routing, budgets | **this `aichat` fork** | **The backlog is almost entirely here** |
| **L3 — Workspace Supervisors** | Terminal multiplexing, HITL gates, worktrees | `dot-agent-deck`, `bohay`, `AoE` | External tools (adoption, not our code) |
| **L4 — Enterprise Control Plane** | Multi-tenant auth, cloud audit, fleet coordination, shared cache | `TrueForge`, **Fleet Commander / "Hive-Mind"** | **Proposed only — not built, mostly out of engine scope** |

Design intent: the engine (L2) is the fixed constant; L3/L4 sit *around* it and drive it via CLI/PTY (L3) or Remote MCP/HTTP/WSS (L4). This is why the roadmap and backlog barely overlap — the roadmap largely describes what surrounds the engine.

---

## Roadmap ↔ Backlog Crosswalk

| Strategic strand | Layer | Status | Backlog item | Notes |
|------------------|-------|--------|--------------|-------|
| Native in-process MCP | L2 | ✅ Shipped | **#1** (Done) | Replaced the Node.js bridge. |
| Provider-agnostic agent loop | L2 | ✅ Shipped | **#3** (Done) | Parallelism, budgets, sub-agents, `_plan`. |
| Declarative stream routing | L2 | ✅ Shipped | **#4** (Done) | Auto-cap, pipes, file targets. |
| Deterministic test/coverage hardening | L2 | ✅ Shipped | **#5** (Done) | agent_loop.rs 46.8%→64.7% line. |
| Tool safety modes / actuation governance | L1/L2 | 🔜 Proposed | **#6** (High) | HITL-adjacent; complements L3 approval gates. |
| Session resumption / WAL | L2 | 🔜 Proposed | **#7** (High) | Durable state; survives dropouts/SIGINT. |
| Context compaction | L2 | 🔜 Proposed | **#8** (Med) | Fallback to the delegation-first hygiene model. |
| Ephemeral Git worktree isolation | L2 | 🔜 Proposed | **#9** (Med) | Engine-side complement to L3 `bohay`. |
| Staged config / dry-run ops | L1 | 🔜 Proposed | **#10** (Med) | SRE actuation safety; mostly `llm-functions`. |
| Mock-client test seam | L2 | 🔜 Proposed | **#11** (Low) | Deterministic coverage of `run()` orchestration. |
| **Remote MCP transports (HTTP/WSS)** | L2→L4 | 🔜 Proposed | **#12** (Med) | The concrete engine work that lets L4 control planes drive the engine remotely. |
| **Scoped shared artifact store** (engine-level cross-agent memory) | L2 | 🔜 Proposed | **#13** (Low) | Structured, root-PID-scoped, read-mostly. The *engine-level* counterpart to the L4 Hive-Mind — NOT a free-form blackboard. |
| Gemini Interactions API | L2 | ⏸ Deferred | **#2** (Low) | Covered via OpenRouter + client loop. |
| CLI flag parity (`--show-trace/--max-turns/--max-cost`) | L2 | ✅ Largely shipped | *(folded into #3)* | Flags exist; no distinct open item. |
| **Adopt `dot-agent-deck`** (SRE mission control, HITL cards) | L3 | 🧭 External adoption | *(none — not our code)* | #1 SRE supervisor recommendation. Engine already emits the status-file/`/dev/tty` signals it consumes. |
| **Adopt `bohay`/Luvus** (worktree multiplexer, file leases) | L3 | 🧭 External adoption | *(overlaps #9)* | #1 code-refactoring supervisor. |
| **Agent of Empires (AoE)** (tmux fleet dashboard) | L3 | 🧭 External option | *(none)* | Host-dependent; lower portability. |
| **Fleet Commander MCP Server / "Hive-Mind"** (cloud shared semantic cache, fleet coordination) | L4 | 💭 Proposed concept only | *(none — see #13 for the engine-level slice)* | **Not built, not enumerated as phases, not in the backlog.** A cloud/fleet aspiration above the engine. |
| **TrueForge / enterprise K8s gateway** (RBAC, cloud audit) | L4 | 💭 External / concept | *(none)* | Out of engine scope entirely. |

Legend: ✅ shipped · 🔜 proposed & tracked in backlog · ⏸ deferred · 🧭 external tool to adopt · 💭 concept only (no backlog item).

---

## Honest Status Notes

- **The "Hive-Mind" / 3-Tier Evolving Semantic RAG Cache is a concept, not code.** It appears only in the Session 1 summary (as a "formulated" idea) and in the SRE-landscape doc (as a *proposed* Layer-4 component). It is **not implemented and not in the backlog.** The only thing that exists today is the per-instance Layer-2 hybrid RAG (`src/rag/`, HNSW + BM25 + RRF) — retrieval, not shared cross-agent memory. Backlog **#13** captures the *engine-level, single-tree* slice of the idea; the cloud/fleet version remains an L4 aspiration.
- **The "6-phase Distributed Fleet" roadmap was never enumerated.** It exists only as a one-line mention in the Session 1 summary. The six phases were not written down anywhere, so they are intentionally not reproduced here rather than fabricated. If a phased plan is wanted, it needs to be authored deliberately.
- **L3/L4 tools are adoption recommendations, not fork deliverables.** `dot-agent-deck`, `bohay`, `AoE`, `TrueForge` are external projects. The engine's job is to emit the right contracts (status files, `/dev/tty` signals, Remote MCP via #12) so they can drive it — not to build them.

---

## Suggested Sequencing (engine work only)

The backlog's own dependency notes still govern ordering. At a strategic level:

1. **High-priority safety/durability next:** #6 (Tool Safety Modes) and #7 (WAL Resumption) — both High, both core to the SRE stress-test.
2. **#12 Remote MCP** when L4 integration becomes concrete (it's the bridge that makes any L4 control plane useful).
3. **#13 shared artifact store** and **#11 mock-client seam** are Low — pick up opportunistically or when a dependent feature (#7, #9) makes them cheap.
4. **#2 Gemini Interactions** stays deferred unless OpenRouter coverage proves insufficient.

Pillar/Tenet alignment for each item is in [`progress.md`](progress.md) (the backlog table) and detailed in [`backlog.md`](backlog.md).
