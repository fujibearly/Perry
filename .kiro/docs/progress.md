# Project Progress

## Current State (2026-09-02)

**Branch:** `feat/tool-safety-6a` (off `main`) — backlog #6a in progress  
**Version:** v0.31.0-fork.9  
**Tests:** 360 tests pass, 0 fail (352 unit + 5 catalog-override + 3 integration) — +8 from backlog #6a (capability mask); +25 earlier from #5 hardening  
**E2E demos (2026-09-02, live):** 11 scenarios executed, 23/24 assertions pass. The one miss — the OSC tmux pane-title update in Demo 6 — requires an interactive tmux pane as the process's controlling `/dev/tty`; it does not land when run nested inside another CLI. The pipe-proof observability path (status file + `/dev/tty` trace) passes.  
**Agent Loop Coverage:** unit-test (`cargo test`) line coverage of `src/agent_loop.rs` rose **46.8% → 64.7%** (+17.9 pts) from the backlog #5 tests — see [coverage re-measurement 2026-09-02](file:///home/istari/projects/aichat/.kiro/docs/coverage-remeasurement-2026-09-02.md). NB: not comparable to the older 72.9% figure, which used the live E2E harness (different methodology — [2026-08-31 report](file:///home/istari/projects/aichat/.kiro/docs/coverage-evaluation-2026-08-31.md)).

## Backlog Status

> Strategic context: [`roadmap.md`](roadmap.md) (4-layer roadmap ↔ backlog crosswalk). Items #5-#10 originate from the Session-2 architecture blueprint ([`architecture-blueprint-2026-08-28-to-2026-09-02.md`](architecture-blueprint-2026-08-28-to-2026-09-02.md)).

| # | Item | Status | Priority | Scope / Branch | Rationale | Alignment to Architecture & Philosophy | Effort |
|---|------|--------|----------|----------------|-----------|----------------------------------------|--------|
| 1 | Rust MCP Bridge | ✓ Done | High | `feat/rust-mcp-bridge` | Foundation. Replaces the Node.js MCP bridge with an in-process Rust client, unblocking tool-ecosystem access for both loops with no new abstractions or runtime dependency. | **Pillar 6 (Portability / Zero-Dependency) + Tenet 5:** removes the Node runtime, keeping the single static musl binary deployable on 64MB bastions. | L — done (~1272 lines) |
| 3 | Client-Side Agent Loop | ✓ Done | High | `feat/agent-loop-enhancements` | The only provider-agnostic orchestration; enhancing it (parallel tools, turn budget, sub-agents, `_plan`) gives *every* provider agentic capability without server-side support. | **Pillars 1, 2, 5:** hierarchical delegation over context monoliths, process-isolated sub-agents (PIDs), turn/cost circuit breakers. Core of the fork thesis. | L — done (multi-phase A–F) |
| 4 | Tool Output Routing | ✓ Done | Medium | `feat/tool-output-routing` | Every tool result currently re-enters LLM context, which is wasteful for large/final outputs; routing to file or pipe makes tool composition practical without burning context. | **Pillar 3 (Declarative Data Flow):** Unix-style pipes, file targets, auto-capping — implements a named pillar directly. | M — done (~200 lines) |
| 5 | Test Suite & Coverage Hardening | ✓ Done | Medium | `feat/test-suite-hardening` | Coverage analysis showed strong baseline but untested edge paths in error handling, crash isolation, cyclic pipe aborts, and budget conditions. | **Pillar 5 (Deterministic Safety):** validates the circuit-breaker/budget guarantees the fork claims. Cross-cutting; hardens existing behavior rather than adding capability. | M — done. +25 unit tests (21 agent_loop + 4 mcp) + Demo 12 (offline sub-agent crash isolation). Coverage: agent_loop.rs 46.8%→64.7% line. FR-4 closed via Demo 12; circuit-breaker/cost logic extracted + tested. Merged to main. |
| 6 | Tool Safety Modes & Actuation Governance (umbrella; specced) | Spec written; #6a in progress | High | `feat/tool-safety-6a…6d` | In system-wide operations, parallel sub-agents must never cause catastrophe; a graduated, safety-governed actuation model gates *which* agent may perform *which* action, with a non-pardonable deterministic floor + LLM risk overlay + escalation to humans. | **Tenet 4 ("triage in parallel, actuate in sequence") + Pillar 5:** the missing enforcement layer for the SRE stress-test. Strong fit. | Umbrella (grew well past the original ~150-250 lines); decomposed into 4 stacked, independently-shippable increments (#6a–#6d) that degrade gracefully. Spec: [`.kiro/specs/tool-safety-modes/`](file:///home/istari/projects/aichat/.kiro/specs/tool-safety-modes/) |
| 6a | ↳ Deterministic capability mask (floor / fallback) | ✓ Implemented (`feat/tool-safety-6a`, unmerged) | High | `feat/tool-safety-6a` | Binary `readonly`/`mutating` mask; sub-agents read-only by default; **unclassified tools reserved to humans**. The permanent floor everything degrades to. | **Pillar 2 + Pillar 5:** capability mask rides the child-PID env channel (like `AICHAT_AGENT_DEPTH`). | S–M — done. `function.rs` (`ToolMode`/`SafetyClass` + `mode` field), `agent_loop.rs` (mask propagation + `capability_denied` gate), `mcp.rs` (MCP tools → unclassified). +8 unit tests; suite 352→360, 0 fail. |
| 6b | ↳ Blast-radius tiers + proven reversibility + Protected Policy File + authority gradient | Proposed | High | `feat/tool-safety-6b` | 5-tier radius (`Safe`→`Catastrophic`), orthogonal *proven* reversibility, non-pardonable Protected Policy File, root-favoring authority ceiling. Fully deterministic. | **Tenet 4 + Pillar 5:** deterministic floor before any LLM; authority grows toward the root (more context). | M — new `src/safety.rs`, config `safety:` section, `function.rs` fields |
| 6c | ↳ `%assess-risk%` LLM evaluator (stricter-only overlay) | Proposed | High | `feat/tool-safety-6c` | Dedicated cheap model + minimal-context role returns a structured verdict that can only make things *stricter*; plan-time pass flags key steps, act-time re-check. | **Principle: "the LLM is not a Pardoner."** Advisory overlay clamped in Rust. | M — role asset + `safety.rs` evaluator/clamp; ~fast-path skip for `Safe` |
| 6d | ↳ Escalation & control protocol + human-in-the-loop | Proposed | High | `feat/tool-safety-6d` | mTLS **WebSocket** inter-agent channel (child dials parent, loopback WSS), typed `Escalation`/`Verdict{Halt\|Revert\|Continue}`/`Cancel` protocol, durable rollback journal (REVERT replays it — survives connection/child death), branch-only suspension, upward propagation to orchestrator, then interactive-CLI human prompt or Layer 3. Forward-compatible with remote agents. | **Pillar 2 (process-isolated control) + Pillar 5.** Control plane = ephemeral WSS; durability plane = on-disk journal; audit plane = #14. | L — WSS listener + mutual-auth handshake + message protocol + rollback journal; live parent↔child control |
| 7 | Session Resumption & WAL Journaling (`--resume`) | Proposed | High | `feat/session-wal-resumption` | Long diagnostic sessions must survive network dropouts, rate-limits, and `SIGINT` without re-running expensive probes. | **Tenet 1 (system-level scope) + Pillar 4 (Observability):** extends out-of-band state (status files → durable WAL). Fits, though it introduces a modest new stateful concept. | L — ~250-350 lines + new `session_wal.rs`; replay/checkpoint correctness is the hard part |
| 8 | Dynamic Multi-Turn Context Compaction | Proposed | Medium | `feat/context-compaction` | Preserves context hygiene (Pillar 1): extended 15+ turn investigations accumulate context monoliths that contaminate reasoning; rolling micro-summaries keep the working context dense. | **Pillar 1 (Delegation over Context Monoliths):** directly named. Tension: the fork's *primary* answer to bloat is delegation/routing; compaction is a complementary in-thread fallback. | M — ~200-300 lines; summarization-quality tuning adds uncertainty |
| 9 | Ephemeral Git Worktree Isolation for Coders | Proposed | Medium | `feat/ephemeral-git-worktrees` | Concurrent `coder` sub-agents in a Git repo must build, edit, and test without file clobbering or build collision. | **Pillar 2 (Process Isolation)** extended to filesystem isolation. **Caveat:** scoped to the coding sub-case, which the philosophy frames as the non-default "worktree trap" — fine while opt-in. | M — ~150-250 lines; worktree lifecycle/cleanup edge cases |
| 10 | Staged Config & Dry-Run Protocol for Ops | Proposed | Medium | `feat/staged-ops-protocol` | Host config mutations (Caddyfile, K8s manifests) require pre-flight syntax validation and rollback safety before live activation. | **Tenet 4 (safe sequential actuation) + Pillar 5:** stage → validate → atomic apply/rollback. Strong fit; largely tooling/prompt-contract convention. | S–M — mostly `llm-functions` tooling + prompt contracts, little engine code |
| 11 | Mock-Client Test Seam for Loop Coverage | Proposed | Low | `feat/mock-client-seam` | Backlog #5 covered the loop's decision helpers, but the `run()` orchestration (turn iteration, streaming, tripped-call dispatch, sub-agent recursion) is gated behind a live LLM and only reached by the billed E2E harness. A mock-client seam enables deterministic, offline coverage of that wiring. | **Pillar 5 (Deterministic Safety):** hardens the core loop with reproducible tests. Follow-on to #5; touches the engine hot path so warrants its own spec. | M — ~150-300 lines; injectable LLM turn source + scripted-turn tests; behavior-preserving refactor risk |
| 12 | Remote MCP Transports (HTTP/WSS) | Proposed | Medium | `feat/remote-mcp-transports` | Native MCP speaks only stdio to local servers; roadmap Layer-4 control planes and remote MCP servers need HTTP/WSS. Concrete L2 engine work unblocking the roadmap's "Remote MCP" arrow. | **Transparent MCP Integration + Pillar 6:** remote tools still appear as normal `FunctionDeclaration`s; stays a single static binary, stdio remains the zero-config default. | M — ~200-400 lines; transport abstraction + auth/reconnect in `src/mcp.rs` |
| 13 | Scoped Shared Artifact Store for Orchestration Trees | Proposed | Low | `feat/shared-artifact-store` | No way for sibling sub-agents in one tree to share intermediate artifacts (dedup expensive fetches, producer→consumer handoff) without round-tripping the parent. Engine-level counterpart to the L4 "Hive-Mind", scoped to a local tree. | **Pillars 1 & 2 (tension, managed):** structured append-only, root-PID-scoped, read-mostly for siblings — deliberately NOT a free-form blackboard, to preserve process isolation & context hygiene. | M — ~150-300 lines; needs its own spec; interacts with #7 (WAL) and #9 (worktrees) |
| 14 | Per-Machine Consolidated Audit Log (auditability) | Proposed | Medium | `feat/audit-log` | Observability today is live/ephemeral/single-process (`/dev/tty`, OSC, per-PID status files that vanish). No durable, consolidated, historical record for an external auditor/SRE/platform to reconstruct what aichat did on a host over time. Auditability is a *distinct goal* from observability. | **Pillar 4 extended to durability + Pillar 1/2:** each isolated agent authors its own append-only JSONL records; consolidated per-machine view via correlation IDs at read time. Distinct *audit plane* (vs. #6d control + rollback planes). | M — ~200-400 lines new writer; also the natural home to fix the `$0.000000` cost bug; needs its own spec |
| 2 | Gemini Interactions API | Deferred | Low | — (Covers via OpenRouter/Client Loop) | Future-proofs against `generateContent` deprecation, but Google's API may still shift and OpenRouter + client loop already cover Gemini agentic use. | **Weakest fit.** Provider-specific server-side integration vs. the fork's provider-agnostic thesis; #3 already makes Gemini agentic, which is why it's deferred. | L — ~1000-1500 lines + new `gemini_interactions.rs`; external API stability risk |

**Effort scale:** S ≈ under ~150 lines / a few hours · M ≈ ~150-400 lines / 1-2 days · L ≈ ~400+ lines or new modules / multi-day. Estimates are relative and derive from the scope figures in [`backlog.md`](file:///home/istari/projects/aichat/.kiro/docs/backlog.md); risk notes flag where correctness or external dependencies widen the range. Pillar/Tenet references map to [`fork-philosophy-and-architecture.md`](file:///home/istari/projects/aichat/.kiro/docs/fork-philosophy-and-architecture.md).

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

## What's Implemented

### Backlog #1: Rust MCP Bridge ✓

In-process Rust MCP replacing Node.js. Transparent integration (Option C), cached manifests, lazy spawn, feature-flagged.

### Backlog #3: Agent Loop Enhancements ✓

Provider-agnostic iterative loop with:
- Parallel tool execution (semaphore-bounded join_all)
- Turn budget (configurable max_turns, stderr warning)
- Planning tool (_plan auto-injected, acknowledged result, trace events)
- Sub-agent subprocess delegation (aichat spawns aichat, depth-bounded)
- Progress rendering (spinner + trace lines, 2s heartbeat)
- External observability (OSC title, JSON status file, BEL + OSC 777 notifications)
- 29 tests covering config, planning, progress, formatting, depth

### Backlog #4: Tool Output Routing ✓

Declarative output routing on FunctionDeclaration:
- Auto-capping: results > tool_output_limit → temp file + preview
- File destination: write to path template, return confirmation
- Pipe destination: chain tools without LLM round-trip, cycle detection
- 16 tests covering capping, file routing, pipe chains, templates, config

### Backlog #6a: Deterministic Capability Mask ✓ (unmerged, `feat/tool-safety-6a`)

First increment of the #6 Tool Safety Modes umbrella:
- `ToolMode { Readonly, Mutating }` + skip-serialized `mode` field on `FunctionDeclaration`; `SafetyClass { Readonly, Mutating, Unclassified }` (absent mode = Unclassified, reserved to humans)
- Sub-agents inherit `AICHAT_CAPABILITY_MASK=readonly`; masked processes refuse mutating/unclassified tools with a structured `capability_denied` result (not a crash)
- MCP tools → unclassified; `_plan` always permitted
- +8 unit tests; suite 352→360 unit, 0 fail; clippy clean
- The permanent safety floor #6b–#6d degrade back to

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

## Branch Status

- `main` — upstream fork at v0.31.0-fork.9 (all prior `feat/*` branches merged here)
- `rc-branch` — release candidate (stale ancestor of `main`)
- `feat/rust-mcp-bridge` — Backlog #1, complete (merged to `main`)
- `feat/agent-loop-enhancements` — Backlog #3, complete (merged to `main`)
- `feat/tool-output-routing` — Backlog #4, complete (merged to `main`)
- `feat/test-suite-hardening` — Backlog #5, complete (merged to `main`)
- **`feat/tool-safety-6a`** — Backlog #6a, **implemented (active, unmerged; 3 commits ahead of `main`)**

## Merge Strategy

```
main ← feat/rust-mcp-bridge ← feat/agent-loop-enhancements ← feat/tool-output-routing (all merged)
main ← feat/tool-safety-6a (current, unmerged)
     ← feat/tool-safety-6b ← …6c ← …6d (future, each off the previous)
```

## Environment Reminders

- Production aichat: `/usr/bin/aichat` (v0.30.0), config at `~/.config/aichat/`
- Dev binary: `~/projects/aichat/target/release/aichat`
- To test: `AICHAT_CONFIG_DIR=/tmp/aichat-test` or use same config (read-only compatible)
- Live functions (don't touch): `~/clones/llm-functions`
- Dev functions (safe): `~/projects/llm-functions`
- pdf2md: installed via `cargo install pdf-inspector`
