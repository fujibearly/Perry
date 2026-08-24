# aichat Architecture

A Rust CLI tool (v0.31.0-fork.9) that provides a unified interface to multiple LLM providers. Authored by sigoden, forked with enhancements for native MCP, a provider-agnostic agent loop, and external observability. Operates in three modes: **command-line** (one-shot queries), **REPL** (interactive chat), and **HTTP server** (exposes OpenAI-compatible APIs).

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
  │   ├─ eval_tool_calls_parallel
  │   │     ├─ semaphore (max_concurrency)
  │   │     ├─ join_all (parallel dispatch)
  │   │     │    ├─ MCP tools → call_mcp_tool_async
  │   │     │    ├─ Agent tools → subprocess (future)
  │   │     │    └─ Shell tools → spawn_blocking + eval_shell
  │   │     └─ results in original order
  │   ├─ merge_tool_results into next input
  │   └─ emit progress events
  │
  budget exhausted → BudgetExhausted, return partial
```

### Key properties

- **Provider-agnostic**: Works with any client returning `tool_calls`
- **Parallel by default**: Multiple tool calls execute concurrently (semaphore-bounded)
- **Bounded**: Configurable `max_turns` (default 20) prevents runaway
- **Observable**: Emits `AgentLoopEvent` for progress rendering, OSC titles, status files
- **Composable**: Sub-agents spawn as separate aichat processes (future Phase E)

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

### Tool dispatch

```
ToolCall arrives from LLM
  │
  ├─ [agent_loop path] eval_tool_calls_parallel → eval_single_tool
  │     ├─ MCP match?  → call_mcp_tool_async (await)
  │     ├─ Agent tool?  → subprocess spawn (future)
  │     └─ Shell tool   → spawn_blocking + eval_shell
  │
  └─ [legacy path] eval_tool_calls_async → eval_single_tool_async
        ├─ MCP match?  → call_mcp_tool_async (await)
        └─ Shell tool   → spawn_blocking + eval_shell
```

Tool calls are deduplicated and infinite loops are detected.

---

## Config System (`src/config/`)

`Config` is wrapped in `Arc<RwLock<Config>>` (aliased as `GlobalConfig`) for shared mutable access across async code.

### Key concepts

- **Roles** — System prompts stored as markdown files. Built-in roles: `%shell%`, `%code%`, `%explain-shell%`, `%create-title%`, `%functions%`. Users can create custom roles.
- **Sessions** — Persistent conversation history with token tracking, auto-compression (summarization when exceeding `compress_threshold`), and auto-naming.
- **Agents** — Full-featured autonomous entities with: their own functions/tools, MCP servers, RAG integration, variables (user-configurable), dynamic instructions (generated at runtime), conversation starters, and per-session state.
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

The REPL's `ask_inner` delegates to `agent_loop::run()` for tool-calling interactions.

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
- **spinner** — progress indicators
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
4. **Sub-agents as subprocesses** — process boundary gives identity, observability, crash isolation
5. **Trait-based polymorphism** — `Client` trait for providers, `RoleLike` trait for prompt sources
6. **Static lazy initialization** — `LazyLock` and `OnceLock` for expensive one-time computations
7. **Transparent MCP integration** — MCP tools are indistinguishable from shell-exec tools to the rest of the codebase
