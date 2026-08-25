# aichat Architecture

A Rust CLI tool (v0.31.0-fork.9) that provides a unified interface to multiple LLM providers. Authored by sigoden, forked with enhancements for native MCP, a provider-agnostic agent loop, and external observability. Operates in three modes: **command-line** (one-shot queries), **REPL** (interactive chat), and **HTTP server** (exposes OpenAI-compatible APIs).

This is not a coding agent. It's a general-purpose LLM CLI — a Swiss army knife for interacting with any provider, with tools, sessions, RAG, roles, and an API server. The agent loop makes tool-calling reliable and efficient for *any* use case: research, data processing, automation, analysis, whatever the user configures.

---

## Design Philosophy

### Upstream (sigoden)

sigoden's design treats aichat as a **thin orchestration layer**: the LLM decides, aichat dispatches, external scripts execute. The intelligence lives in the LLM and the tools — aichat is the pipe between them.

Key principles:
- Tools are tools. A function is a shell script with a JSON schema. Nothing more.
- Roles are behavioral presets. A prompt in a markdown file that changes how the LLM responds.
- Agents compose these. Agent = Instructions (Prompt) + Tools (Function Calling) + Documents (RAG).

### Fork additions

The fork preserves this philosophy but adds **runtime intelligence to the dispatch layer**:
- **Parallelism** — the harness knows tool calls are independent and executes them concurrently
- **Budgets** — the harness knows to stop after N turns, preventing runaway
- **Planning** — the harness gives the model a reasoning channel (`_plan`) without polluting output
- **Observability** — the harness reports what's happening (OSC titles, status files, notifications)
- **Delegation** — the harness can spawn other agents as subprocesses with independent lifecycles

The LLM and the scripts are unchanged. The pipe between them got smarter.

### What changed vs. what didn't

| Aspect | Unchanged | Enhanced |
|--------|-----------|----------|
| Agent definition format | `index.yaml` + `functions.json` + RAG | — |
| Role format | Markdown with optional front-matter | — |
| Function format | Shell script + JSON schema | — |
| Tool invocation | `run_command()` with arguments | Parallel, bounded, observable |
| Agent composition | `agent: true` flag on tools | Now spawns full aichat subprocess (not just shell-exec) |
| Orchestration logic | Lives in the LLM's prompt | Same — prompt IS the orchestration strategy |
| Provider support | 7 native + 18 compatible | Same |

---

## The Role / Function / Agent Hierarchy

```
Role       = prompt + model config + use_tools selector
Session    = role + conversation history + compression
Agent      = role + own tools + own MCP servers + own RAG + variables + dynamic instructions
```

The unifying trait is `RoleLike` — everything resolves to: what model, what temperature, what tools.

**Functions** (tools) are a global pool defined in `functions.json`. A role or agent *selects from* that pool via `use_tools: "fs,web_search"`. Functions themselves are dumb shell scripts — they have no awareness of who called them.

**Agents** are the composition layer. An agent with `agent: true` tools can delegate to other agents, forming a tree:

```
User
 └─ aichat --agent orchestrator "do X"     (depth 0)
      ├─ _plan: "I'll research first, then implement"
      ├─ aichat --agent researcher "find Y"   (depth 1)
      │    └─ web_search, fetch_url (parallel)
      └─ aichat --agent implementer "build Z" (depth 1)
           └─ fs_write, execute_command
```

Each agent in the tree is a full aichat instance: own PID, own turn budget, own tools, own session, own status file. The depth is bounded by `max_agent_depth` (default 3) via `AICHAT_AGENT_DEPTH` env var.

---

## Entry Point (`main.rs`)

Determines the **working mode** based on CLI args:
- `--serve` → `WorkingMode::Serve` — starts an HTTP server
- No text and no files → `WorkingMode::Repl` — interactive session
- Otherwise → `WorkingMode::Cmd` — one-shot execution

The core flow for command mode is `start_directive()`, which:
1. Creates a client from the input
2. If multi-agent is enabled → `run_multi_agent_directive()` (OpenAI Responses path)
3. Otherwise → `run_directive()` → `agent_loop::run()` (provider-agnostic path)

---

## Agent Loop (`src/agent_loop.rs`)

The provider-agnostic iterative agent loop. Replaces the old `#[async_recursion]` pattern with a bounded `for turn in 1..=max_turns` loop.

### Flow

```
agent_loop::run(input, params)
  │
  for turn in 1..=max_turns:
  │   ├─ call_llm_raw (streaming or non-streaming)
  │   ├─ if no tool_calls → LoopComplete, return
  │   ├─ partition: _plan calls vs real tool calls
  │   │     └─ _plan: emit PlanReceived, return "acknowledged"
  │   ├─ eval_tool_calls_parallel (real calls only)
  │   │     ├─ semaphore (max_concurrency)
  │   │     ├─ join_all (parallel dispatch)
  │   │     │    ├─ MCP tools → call_mcp_tool_async
  │   │     │    ├─ Agent tools → eval_agent_tool_subprocess
  │   │     │    └─ Shell tools → spawn_blocking + eval_shell
  │   │     └─ results in original order
  │   ├─ merge_tool_results into next input
  │   └─ emit progress events
  │
  budget exhausted → BudgetExhausted, return partial
```

### Key properties

- **Provider-agnostic**: Works with any client returning `tool_calls` — OpenRouter, Claude, Cohere, DeepSeek, Ollama, any OpenAI-compatible endpoint
- **Parallel by default**: Multiple tool calls execute concurrently (semaphore-bounded at `max_concurrency`)
- **Bounded**: Configurable `max_turns` (default 20) prevents runaway — the model can't loop forever
- **Observable**: Emits structured `AgentLoopEvent` for progress rendering, OSC titles, status files, notifications
- **Composable**: Sub-agents spawn as separate aichat processes with independent lifecycles, recursively orchestrable
- **Planning-aware**: Built-in `_plan` pseudo-tool lets the model reason before acting without polluting output

### Two loop paths (peers, not competitors)

The server-side loop (OpenAI Responses multi-agent) and the client-side loop (agent_loop) coexist:
- **Server-side**: Lower latency when the provider supports it (provider-managed state). OpenAI-only.
- **Client-side**: Universal fallback that makes *every* provider agentic. Provider-agnostic.

They share the tool execution layer but have different delegation models (who decides what to call next).

### Configuration (`agent_loop` in config.yaml)

```yaml
agent_loop:
  max_turns: 20           # Turn budget
  max_concurrency: 8      # Parallel tool execution limit
  max_agent_depth: 3      # Sub-agent nesting depth
  show_trace: false       # Print trace events to stderr
  planning_tool: true     # Inject _plan pseudo-tool
  osc_title: true         # Terminal title updates
  status_file: true       # JSON status file for external tools
  notify: true            # BEL + OSC 777 on completion
  tool_output_limit: 16384  # Large result capping threshold (bytes)
  workflow_tool: true     # Multi-phase fan-out tool
```

### External observability (designed for tmux)

The agent loop emits signals for external management tools (tmux, Herdr, Agent Deck) without parsing stdout:

- **OSC 0/2 terminal title** — live state: `aichat: turn 3/20 | fs_write, execute_command`
- **JSON status file** — `$XDG_RUNTIME_DIR/aichat-<pid>.json`, one per process (including sub-agents)
- **BEL + OSC 777** — desktop notifications on completion (tmux `monitor-bell`, Ghostty/iTerm2 native)

Each sub-agent process writes its own independent status file. External tools enumerate `aichat-*.json` for a fleet view. No coordination between processes needed — each owns its own signals.

---

## Client System (`src/client/`)

Uses a macro-based registration system (`register_client!`) to define all supported providers.

### Native clients
OpenAI, Gemini, Claude, Cohere, Azure OpenAI, VertexAI, Bedrock

### OpenAI-compatible providers (18 total)
ai21, cloudflare, deepinfra, deepseek, ernie, github, groq, hunyuan, minimax, mistral, moonshot, openrouter, perplexity, qianwen, xai, zhipuai, jina, voyageai

### Client trait
Each client implements the `Client` trait with methods for:
- `chat_completions` (non-streaming)
- `chat_completions_streaming` (SSE-based)
- `embeddings` (for RAG)
- `rerank` (for RAG result re-ranking)

### LLM call variants

| Function | Returns | Used by |
|----------|---------|---------|
| `call_chat_completions` | `(Output, Vec<ToolResult>)` | Legacy callers (shell-execute mode) |
| `call_chat_completions_streaming` | `(Output, Vec<ToolResult>)` | Legacy callers |
| `call_chat_completions_raw` | `(Output, Vec<ToolCall>)` | Agent loop (handles eval itself) |
| `call_chat_completions_streaming_raw` | `(Output, Vec<ToolCall>)` | Agent loop |

### Macro-generated functions
The `register_client!` macro auto-generates:
- `init_client()` — picks the right client based on model
- `list_all_models()` / `list_models()` — discovers available models
- `create_client_config()` — interactive setup

Models are defined statically in `models.yaml` (bundled) but can be overridden via `--sync-models`.

---

## MCP Bridge (`src/mcp.rs`)

Native Rust MCP client replacing the Node.js bridge. Behind `mcp` cargo feature flag (default on).

### Key design

- MCP tools appear as regular `FunctionDeclaration` entries — transparent to LLM, agents, and `use_tools`
- Cached manifests at `<config-dir>/mcp-cache/<server>.json` (invalidated by config hash)
- Lazy server spawn on first tool invocation
- Connection pool: one connection per server, parallel calls spawn additional connections
- Namespaced: `server__tool` (double underscore separator)
- `--sync-mcp` CLI flag for forced refresh

### Async paths

| Function | Used by |
|----------|---------|
| `call_mcp_tool` (sync, block_in_place) | OpenAI Responses module's `execute_function_calls` |
| `call_mcp_tool_async` (async, direct) | Agent loop's `eval_single_tool` |

---

## Function Calling (`src/function.rs`)

Tools are external scripts/binaries:
- Declarations live in `functions.json` (JSON schema format)
- Binaries live in `functions/bin/`
- Agents can have their own function directories
- MCP tools are merged into the same declaration list
- `_plan` pseudo-tool is auto-injected when `planning_tool: true`

### Tool dispatch (three routes)

```
ToolCall arrives from LLM
  │
  ├─ _plan?  → return "acknowledged" (handled in loop, not dispatched)
  │
  ├─ MCP match?  → call_mcp_tool_async (await, no blocking)
  │
  ├─ agent: true? → eval_agent_tool_subprocess
  │     └─ spawn: aichat --agent <name> "<task>"
  │        (AICHAT_AGENT_DEPTH incremented, own PID/budget/status)
  │
  └─ Shell tool → spawn_blocking + eval_shell
       └─ run_command(bin_name, args, envs)
```

Tool calls are deduplicated and infinite loops are detected (before dispatch).

---

## Config System (`src/config/`)

`Config` is wrapped in `Arc<RwLock<Config>>` (aliased as `GlobalConfig`) for shared mutable access across async code.

### Key concepts

- **Roles** — System prompts stored as markdown files. Built-in roles: `%shell%`, `%code%`, `%explain-shell%`, `%create-title%`, `%functions%`. Users can create custom roles. A role is purely behavioral — it changes *how* the LLM responds, not *what tools* it has access to (that's `use_tools`).
- **Sessions** — Persistent conversation history with token tracking, auto-compression (summarization when exceeding `compress_threshold`), and auto-naming.
- **Agents** — Full-featured autonomous entities with: their own functions/tools, MCP servers, RAG integration, variables (user-configurable), dynamic instructions (generated at runtime), conversation starters, and per-session state. An agent spawned as a sub-agent gets its own process, turn budget, and observability.
- **Macros** — YAML-defined multi-step command sequences with interpolated variables.

The `RoleLike` trait unifies Role, Session, and Agent so the system can extract model/temperature/top_p/tools uniformly.

### Config sections

| Section | Purpose |
|---------|---------|
| `model`, `temperature`, `top_p` | LLM defaults |
| `function_calling`, `use_tools`, `mapping_tools` | Tool configuration |
| `mcp_servers` | MCP server definitions |
| `agent_loop` | Agent loop behavior (turns, parallelism, observability) |
| `multi_agent` | OpenAI Responses multi-agent orchestration |
| `rag_*` | RAG configuration |
| `clients` | Provider credentials and settings |

---

## RAG System (`src/rag/`)

A hybrid retrieval system combining:
- **Vector search** — HNSW index (cosine distance) built from embeddings
- **Keyword search** — BM25 engine
- **Reciprocal Rank Fusion** — merges results from both, optionally with a reranker model

Documents are chunked using a recursive character text splitter (configurable chunk_size/overlap). Supports loading from local files, URLs, recursive URL crawling, and custom loader protocols (e.g., `pdftotext`, `pandoc`).

---

## REPL (`src/repl/`)

Built on `reedline` with:
- 36 dot-commands (`.role`, `.session`, `.agent`, `.rag`, `.macro`, `.set`, `.file`, etc.)
- Tab completion with fuzzy matching
- Syntax highlighting
- Vi and Emacs keybinding modes
- Multi-line input (`::: ... :::`)
- Buffer editing with external editor (Ctrl+O)

The REPL's `ask_inner` delegates to `agent_loop::run()` for all interactions. When tools are configured, the loop handles parallel execution, budgets, and observability. When no tools are configured, the loop runs a single turn and returns — functionally identical to a plain LLM call.

---

## HTTP Server (`src/serve.rs`)

Exposes an OpenAI-compatible API using `hyper`:
- `POST /v1/chat/completions` — with streaming (SSE) and non-streaming
- `POST /v1/embeddings`
- `POST /v1/rerank`
- `GET /v1/models`
- `GET /v1/roles`, `/v1/rags`, `/v1/rags/search`
- Web UI at `/playground` and `/arena`

Supports CORS, tool calling in both stream and non-stream modes.

---

## Rendering (`src/render/`)

Markdown rendering with `syntect` for syntax highlighting. Handles:
- Light/dark theme detection via `terminal-colorsaurus`
- Streaming markdown output (progressive rendering)
- Code block highlighting
- Text wrapping

---

## Utilities (`src/utils/`)

- **abort_signal** — cooperative cancellation via Ctrl+C
- **clipboard** — cross-platform copy (arboard)
- **command** — shell execution
- **crypto** — SHA-256 hashing
- **html_to_md** — HTML scraping and conversion
- **loader** — document loading (files, URLs, recursive crawling, protocol loaders)
- **spinner** — progress indicators (2-second heartbeat for agent loop, avoids CPU churn)
- **variables** — environment variable interpolation in prompts
- **render_prompt** — REPL prompt template rendering with colors

---

## Shell Execute Mode (`-e` flag)

Translates natural language to shell commands, then offers interactive options: execute, revise, describe, copy, or quit. Saves successful commands to shell history.

---

## Key Design Patterns

1. **Macro-heavy client registration** — avoids boilerplate for each provider
2. **`GlobalConfig` (Arc<RwLock<Config>>)** — shared mutable state across async tasks
3. **Iterative agent loop with parallel dispatch** — bounded turns, semaphore-controlled concurrency, provider-agnostic
4. **Sub-agents as subprocesses** — process boundary gives identity, observability, crash isolation, and recursive orchestration (orchestrator → sub-orchestrator → workers)
5. **Trait-based polymorphism** — `Client` trait for providers, `RoleLike` trait for prompt sources
6. **Static lazy initialization** — `LazyLock` and `OnceLock` for expensive one-time computations
7. **Transparent MCP integration** — MCP tools are indistinguishable from shell-exec tools to the rest of the codebase
8. **Definitions unchanged, runtime enhanced** — the same `index.yaml` + `functions.json` + RAG definitions run through a fundamentally better engine without any format changes
