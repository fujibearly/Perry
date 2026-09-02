# Project Progress

## Current State (2026-08-31)

**Branch:** `feat/tool-output-routing` (off `feat/agent-loop-enhancements`)  
**Version:** v0.31.0-fork.9  
**Tests:** 319 unit tests pass, 0 fail (11/11 E2E multi-agent demos pass)  
**Agent Loop Coverage:** 72.9% line / 79.4% function ([coverage report](file:///home/istari/projects/aichat/.kiro/docs/coverage-evaluation-2026-08-31.md))

## Backlog Status

| # | Item | Status | Priority | Scope / Branch |
|---|------|--------|----------|----------------|
| 1 | Rust MCP Bridge | ✓ Done | High | `feat/rust-mcp-bridge` |
| 3 | Client-Side Agent Loop | ✓ Done | High | `feat/agent-loop-enhancements` |
| 4 | Tool Output Routing | ✓ Done | Medium | `feat/tool-output-routing` |
| 5 | Test Suite & Coverage Hardening | Proposed | Medium | `feat/test-suite-hardening` |
| 6 | Declarative Tool Safety Modes (`# @meta mode`) | Proposed | High | `feat/tool-safety-modes` |
| 7 | Session Resumption & WAL Journaling (`--resume`) | Proposed | High | `feat/session-wal-resumption` |
| 8 | Dynamic Multi-Turn Context Compaction | Proposed | Medium | `feat/context-compaction` |
| 9 | Ephemeral Git Worktree Isolation for Coders | Proposed | Medium | `feat/ephemeral-git-worktrees` |
| 10 | Staged Config & Dry-Run Protocol for Ops | Proposed | Medium | `feat/staged-ops-protocol` |
| 2 | Gemini Interactions API | Deferred | Low | — (Covers via OpenRouter/Client Loop) |

## Commit History

### `feat/tool-output-routing` (off `feat/agent-loop-enhancements`)

1. `8d3f921` — feat: switch default PDF loader to pdf2md (structured Markdown)
2. `d3c9423` — docs: add spec for tool output routing
3. `9b0794a` — feat: tool output routing — capping, file destination, pipe chains

### `feat/agent-loop-enhancements` (off `feat/rust-mcp-bridge`)

1. `65ef41c` — Phase A: config, module skeleton, async eval, raw LLM call
2. `aeff42b` — Phase B: iterative loop replaces recursion, turn budget enforced
3. `012a7a2` — docs: .kiro specs/steering
4. `ae68429` — Phase C: parallel tool execution
5. `45ed50d` — docs: architecture and progress update
6. `6b46a38` — docs: Fork Enhancements in README
7. `c48f0d5` — Phase D: observability and progress rendering
8. `841e582` — Phase E: planning tool and sub-agent subprocess
9. `73ca1a5` — Phase F: polish, --info display, tests
10. `3cbc2aa` — docs: enriched architecture with design philosophy

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

- `main` — upstream fork at v0.31.0-fork.9
- `rc-branch` — release candidate
- `feat/rust-mcp-bridge` — Backlog #1, complete
- `feat/agent-loop-enhancements` — Backlog #3, complete (Phases A-F)
- `feat/tool-output-routing` — Backlog #4, complete (active)

## Merge Strategy

```
main ← feat/rust-mcp-bridge ← feat/agent-loop-enhancements ← feat/tool-output-routing
```

Each branch builds on the previous. Merge in order.

## Environment Reminders

- Production aichat: `/usr/bin/aichat` (v0.30.0), config at `~/.config/aichat/`
- Dev binary: `~/projects/aichat/target/release/aichat`
- To test: `AICHAT_CONFIG_DIR=/tmp/aichat-test` or use same config (read-only compatible)
- Live functions (don't touch): `~/clones/llm-functions`
- Dev functions (safe): `~/projects/llm-functions`
- pdf2md: installed via `cargo install pdf-inspector`
