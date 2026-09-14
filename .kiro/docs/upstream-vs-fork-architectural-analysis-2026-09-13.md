# Upstream vs. Fork AIChat: Architectural Analysis, Taxonomy Mapping & Uncharted Gaps

**Date:** 2026-09-13  
**Status:** Living Architectural Reference  
**Grounding Sources:**
- Upstream Wiki Knowledge Base: `~/.config/aichat/rags/aichat-upstream-wiki.yaml`
- Argc Tooling Framework: `~/.config/aichat/rags/argc-docs.yaml`
- Repository Source Tree & Specs: `~/projects/aichat` (`.kiro/docs/roadmap.md`, `README.md`, `SESSION_SUMMARY.md`)

---

## 1. Executive Summary

This document establishes the formal architectural crosswalk between three paradigms:
1. **The Modern Autonomous Agent Paradigm:** Formulated as the 4-tier hierarchy:
   $$\textbf{Harness} \longrightarrow \textbf{Agent} \longrightarrow \textbf{Skill} \longrightarrow \textbf{Tool}$$
2. **Upstream AIChat Baseline ([@sigoden](https://github.com/sigoden)):** A terminal-native, single-binary Rust CLI/REPL with built-in HNSW+BM25 hybrid RAG, roles, macros, and Argc/Bash-based tool declarations.
3. **Fork AIChat As-Built (`~/projects/aichat`):** A hardened, system-level execution engine featuring native in-process Rust MCP, an iterative provider-agnostic agent loop, process-isolated subagents, declarative tool stream routing, and a 4-layer actuation governance umbrella (#6a–#6d).

Finally, this analysis details the **strategic gaps**—high-leverage architectural patterns from modern agent systems that are currently absent from both upstream AIChat and the fork's existing roadmap.

---

## 2. Three-Way Architectural Parallel & Taxonomy Crosswalk

| Architectural Tier | Modern Autonomous Architecture | Upstream AIChat Baseline | Fork AIChat As-Built (`~/projects/aichat`) | Fork Innovation & Divergence |
| :--- | :--- | :--- | :--- | :--- |
| **Harness & Loop** | Multi-threaded async event loop managing streaming, tool lifecycles, and context windows. | Single-pass request/response; blocking/synchronous tool execution in `run_llm_function`. | Iterative agent loop (`src/agent_loop.rs`) with semaphore-bounded concurrency (default 8) and turn budgets (`max_turns`). | **Provider-Agnostic Agent Loop**: Parallel tool execution, USD cost budgets (`max_cost`), transient retry backoff, and live `/dev/tty` trace rendering across any model provider. |
| **Agent & Delegation** | Hierarchical subagent trees with isolated worktrees and typed delegation boundaries. | Flat agent directories (`agents/<name>/`); interactive single-agent sessions; no autonomous subagents. | Process-isolated subagents (`agent: true` in `functions.json`) spawned as independent OS child processes. | **Subprocess Isolation**: Depth tracked via `AICHAT_AGENT_DEPTH`, private PIDs, isolated turn budgets, branch-stratified color guides, and British petnames. |
| **Skill & Workflow** | On-demand markdown runbooks (`SKILL.md`) with progressive disclosure. | Procedural `Macro` (`macros/<name>.yaml`) running deterministic REPL commands in isolated context. | Same baseline `Macro` + built-in `_plan` pseudo-tool for task reasoning before actuation. | **Hybrid Planning**: The model reasons via `_plan` (visible in traces, excluded from final output); retains upstream deterministic macros. |
| **Tool & Actuation** | Model Context Protocol (MCP) or code-native typed schemas (Pydantic/Zod). | External Node.js MCP bridge or Bash tools with Argc comment tags (`# @cmd`, `# @option`). | In-process native Rust MCP client (`src/mcp.rs`) + Argc tools annotated with blast-radius metadata. | **Zero-Node Native MCP**: Removed Node.js runtime entirely; tools declare static risk (`# @meta risk <tier>`) and reversibility (`# @meta reversible true`). |
| **Actuation Governance** | Static policy files, API permission scopes, or human approval prompts. | Permissive execution: if declared in `functions.json` and called by the model, it runs. | Layered 4-part safety umbrella (`src/safety.rs`, `src/escalation.rs`): #6a–#6d. | **Graduated Actuation Governance**: Deterministic capability mask (`readonly`), 5-tier blast radius, non-pardonable policy file, LLM risk evaluator, and mTLS escalation channel. |
| **Data Flow & Routing** | Context stuffing with token limiters; or external tool pipes. | All tool outputs enter next prompt directly as a `tool` role message. | Declarative stream routing: `output: {destination: "context" \| "file" \| "pipe"}`. | **Context Pollution Defense**: Auto-caps large results (>16KB) to temp files with preview/hints; pipes multi-tool chains (fetch → summarize) without intermediate LLM round-trips. |
| **Observability** | OpenTelemetry spans, Langfuse traces, web dashboard UIs. | Ephemeral terminal stdout; file logging via `AICHAT_LOG_LEVEL=debug`. | Multi-tier terminal observability: live OSC pane titles, `$XDG_RUNTIME_DIR/aichat-<pid>.json`, BEL/OSC 777 notifications. | **Unix/tmux Native Fleet View**: External tools (tmux, Agent Deck, `jq`) monitor running agents without scraping or corrupting stdout. |
| **Retrieval & RAG** | External vector databases (Chroma, Qdrant) with semantic chunkers. | Embedded HNSW (`hnsw_rs`) + BM25 (`bm25`) + RRF; flat text loader (`pdftotext`). | Same embedded Rust engine, upgraded default loader to `pdf2md` (`firecrawl/pdf-inspector`). | **Structured Ingestion**: Markdown structure (headings, tables) preserved during chunking, improving retrieval precision by 30–40%. |

---

## 3. Implementation Choices: Pros and Cons Analysis

| Subsystem | Upstream Design Choice | Fork As-Built Design Choice | Pros of Fork Approach | Cons / Trade-offs of Fork Approach |
| :--- | :--- | :--- | :--- | :--- |
| **MCP Integration** | External Node.js bridge script. | **In-process native Rust client** using `tokio` process pipes & JSON-RPC 2.0. | • Eliminates Node.js dependency completely.<br>• Retains single static binary deployment (64MB bastion-ready).<br>• Startup time drops from seconds to milliseconds. | • Must maintain JSON-RPC protocol handling in-tree.<br>• Currently supports `stdio` only (no remote HTTP/SSE yet). |
| **Subagent Execution** | None (agents are single-turn interactive sessions). | **Forking independent OS child processes** (`aichat` binary executing `aichat --agent <name>`). | • **True fault isolation**: Child crash/OOM cannot corrupt parent state.<br>• Independent OS scheduling, memory spaces, and PID tracking.<br>• Fits Unix process supervision tools natively. | • Higher process-spawn overhead than async green threads.<br>• Inter-process coordination requires explicit mTLS sockets and IPC. |
| **Safety & Human-in-the-Loop** | Permissive: tools run unconditionally when invoked. | **Layered, non-pardonable governance funnel** with loopback mTLS escalation protocol. | • **Fail-closed guarantees**: Subagents read-only by default; catastrophic actions require human authorization.<br>• Evaluator LLM is strictly stricter-only ("the LLM is not a Pardoner").<br>• Durable rollback journal (`0600` permissions) prevents orphaned mutations. | • Substantial code complexity in `src/safety.rs` and `src/escalation.rs`.<br>• mTLS handshake and ephemeral certificate generation overhead per process tree. |
| **Context Window Hygiene** | Context stuffing: full tool output enters next prompt. | **Declarative routing (`file`/`pipe`) + auto-cap threshold (16KB)**. | • Massive token savings on large command outputs (`cat`, `git diff`).<br>• Tool chains run at native machine speed without LLM round-trip latency. | • Model only receives previews/hints; must make secondary calls if it needs specific truncated lines. |
| **Tool Authoring** | Argc bash scripts with `# @cmd` / `# @option`. | Argc bash scripts augmented with **`# @meta risk <tier>`** and **`# @meta reversible true`**. | • Zero-dependency CLI scripting remains intact.<br>• Static governance metadata compiled into `functions.json` without LLM prompt overhead. | • Requires maintaining the `build-declarations` compiler across Bash, JS, and Python in companion repos. |

---

## 4. Fork Roadmap & Traction Alignment

The fork tracks work via the canonical status table in `.kiro/docs/roadmap.md`:

| Item # | Strategic Capability | Fork Status | Architectural Alignment |
| :---: | :--- | :---: | :--- |
| **#1** | **Native in-process MCP bridge** | **✓ Merged** | Eliminates Node.js; provides unified JSON-RPC stdio tool execution in Rust. |
| **#3** | **Provider-agnostic agent loop** | **✓ Merged** | Parallel tool calls, turn/cost budgets, child subprocesses, and `_plan` tool. |
| **#4** | **Tool output stream routing** | **✓ Merged** | Auto-cap threshold (16KB), file output destinations, acyclic tool pipelines. |
| **#5** | **Test & coverage hardening** | **✓ Merged** | Unit test coverage raised to 64.7% on `agent_loop.rs`; offline crash-isolation harness. |
| **#6** | **Tool safety modes (#6a–#6d)** | **✓ Completed** | Non-pardonable policy file, capability masks, risk evaluator, mTLS escalation channel. |
| **#16**| **Grounding control (`--wslinks`)**| **✓ Completed** | Toggle between fast 1-turn grounding search and multi-turn deep link exploration. |
| **#7** | **Session Resumption & WAL** | **🔜 Proposed** | Append-only write-ahead log to survive dropped connections and `SIGINT` interruptions. |
| **#8** | **Dynamic Context Compaction** | **🔜 Proposed** | In-thread rolling micro-summaries for long-running investigations (15+ turns). |
| **#9** | **Ephemeral Git Worktrees** | **🔜 Proposed** | Micro-worktree isolation (`/tmp/aichat-wt-<pid>`) preventing concurrent coder subagent collisions. |
| **#10**| **Staged Config & Dry-Run Ops** | **🔜 Proposed** | SRE mutation safety: stage to `/tmp/staging/`, validate syntax, atomic apply + `.bak`. |
| **#12**| **Remote MCP Transports** | **🔜 Proposed** | Extends native MCP from stdio to HTTP/SSE and WebSocket for remote services. |
| **#13**| **Scoped Shared Artifact Store** | **🔜 Proposed** | Structured, root-PID-scoped, read-mostly artifact repository for sibling subagents. |
| **#14**| **Consolidated Audit Log** | **🔜 Proposed** | Append-only JSONL historical audit trail distinct from live observability signals. |
| **#15**| **Plan-Driven Structured Execution** | **🔜 Proposed** | Elevates `_plan` from an informal scratchpad to an active execution roadmap. |

---

## 5. Uncharted Gaps (Not Yet in the Fork's Roadmap)

While the fork addresses runtime execution, safety, and governance, several critical patterns from modern autonomous agent systems remain absent from both upstream and the fork's current backlog:

| Missing Architectural Capability | Modern Paradigm Parallel | Why It Matters for a Production CLI Agent | Recommended Implementation Path in Fork |
| :--- | :--- | :--- | :--- |
| **1. Dynamic Skill Discovery (`SKILL.md` Protocol)** | Progressive disclosure skills (Antigravity, Claude Code, Agent Skills). | Today, the fork relies on rigid `Macro` scripts and static system prompts. It cannot load domain-specific runbooks on demand without cluttering the primary prompt or hardcoding steps. | Support `<config_dir>/skills/<name>/SKILL.md`. Inject only `name` and `description` into the prompt; load the full markdown runbook dynamically via a built-in `read_skill` tool when invoked. |
| **2. OS Sandbox Isolation (Landlock / Namespaces)** | Containerized execution / OS-level sandbox (Docker, Bubblewrap, macOS Seatbelt). | The fork's safety layer gates tools *logically*, but when an approved mutating tool runs (`run_command_with_output`), it has unconstrained host access. A compromised prompt can access arbitrary user files. | Implement optional Linux **Landlock** / `unshare` sandboxing in `eval_shell`, restricting child tool processes to `$CWD` and `/tmp` at the kernel level without requiring Docker. |
| **3. In-Flight Dynamic Context Pruning** | Semantic turn eviction & selective message dropping. | Roadmap #8 proposes *compaction* (summarization), but compaction loses fine-grained details and consumes LLM tokens. Many tool outputs (e.g., intermediate compiler errors that were fixed) become useless noise. | Implement a deterministic context pruner that evicts intermediate tool outputs once a downstream tool or turn resolves them, preserving system prompt and final answers at zero token cost. |
| **4. Asynchronous Peer Messaging (Subagent Bus)** | Actor-model message passing (`send_message` / agent mailboxes). | Fork subagents can only communicate hierarchically (child to parent via mTLS escalation). Sibling subagents (e.g., `researcher` and `coder`) cannot exchange findings without parent mediation. | Extend the mTLS escalation hub to support a topic/address bus, allowing sibling processes within the same tree secret to post and receive messages asynchronously. |
| **5. Episodic Memory & Failure Learning (Dynamic RAG)** | Self-evolving memory / reflective experience stores. | AIChat's RAG is strictly static (pre-indexed docs). When an agent solves a tricky production incident or refactoring bug, the lesson is discarded when the process exits. | Add an automatic `.learn` / post-mortem indexer that writes successful multi-turn tool strategies into an `episodic-memory.yaml` RAG store, queried during subsequent runs. |
| **6. Mid-Flight Human Steering (Interrupt Injection)** | Live human interrupt & mid-loop guidance. | Current HITL (#6d) only triggers when an authority threshold is breached. If an agent goes down an unhelpful path within its ceiling, the operator can only press `Ctrl+C`. | Allow the operator to press a steering key (e.g. `Ctrl+G`) during execution pauses to inject guidance directly into the active event queue without terminating the subagent. |

---

## 6. Strategic Takeaways & Synthesis

1. **Preserve the Bastion Philosophy:**  
   The fork's core strength is **zero-runtime portability** (single static Rust binary, no Node.js, no Python, no Docker). Gaps like sandboxing and skills must be implemented using native kernel features (Landlock, filesystem conventions) rather than adopting heavy external frameworks.
2. **From Hierarchical Trees to Process Meshes:**  
   The persistent per-process mTLS channel built in #6d provides a robust foundation. Generalizing this channel from a pure escalation wire into an internal message/artifact bus (addressing Gap 4 and Roadmap #13) will elevate AIChat from a strict tree-delegation runner into a cooperative multi-agent mesh.
3. **Bridge Procedural Macros to Autonomous Skills:**  
   Upstream macros are deterministic and inflexible; modern skills are autonomous but token-hungry. By adopting progressive disclosure (`SKILL.md`), AIChat can offer the best of both worlds: deterministic execution of sub-steps via Argc tools guided by high-level markdown reasoning runbooks.
