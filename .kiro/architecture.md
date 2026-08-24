# aichat Architecture

A Rust CLI tool (v0.30.0) that provides a unified interface to multiple LLM providers. Authored by sigoden, it operates in three modes: **command-line** (one-shot queries), **REPL** (interactive chat), and **HTTP server** (exposes OpenAI-compatible APIs).

---

## Entry Point (`main.rs`)

Determines the **working mode** based on CLI args:
- `--serve` → `WorkingMode::Serve` — starts an HTTP server
- No text and no files → `WorkingMode::Repl` — interactive session
- Otherwise → `WorkingMode::Cmd` — one-shot execution

The core flow for command mode is `start_directive()`, which:
1. Creates a client from the input
2. Calls chat completions (streaming or not)
3. If the response includes tool calls, recursively merges them and calls again

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

### Macro-generated functions
The `register_client!` macro auto-generates:
- `init_client()` — picks the right client based on model
- `list_all_models()` / `list_models()` — discovers available models
- `create_client_config()` — interactive setup

Models are defined statically in `models.yaml` (bundled) but can be overridden via `--sync-models`.

---

## Config System (`src/config/`)

`Config` is wrapped in `Arc<RwLock<Config>>` (aliased as `GlobalConfig`) for shared mutable access across async code.

### Key concepts

- **Roles** — System prompts stored as markdown files. Built-in roles: `%shell%`, `%code%`, `%explain-shell%`, `%create-title%`, `%functions%`. Users can create custom roles.
- **Sessions** — Persistent conversation history with token tracking, auto-compression (summarization when exceeding `compress_threshold`), and auto-naming.
- **Agents** — Full-featured autonomous entities with: their own functions/tools, RAG integration, variables (user-configurable), dynamic instructions (generated at runtime), conversation starters, and per-session state.
- **Macros** — YAML-defined multi-step command sequences with interpolated variables.

The `RoleLike` trait unifies Role, Session, and Agent so the system can extract model/temperature/top_p/tools uniformly.

---

## RAG System (`src/rag/`)

A hybrid retrieval system combining:
- **Vector search** — HNSW index (cosine distance) built from embeddings
- **Keyword search** — BM25 engine
- **Reciprocal Rank Fusion** — merges results from both, optionally with a reranker model

Documents are chunked using a recursive character text splitter (configurable chunk_size/overlap). Supports loading from local files, URLs, recursive URL crawling, and custom loader protocols (e.g., `pdftotext`, `pandoc`).

---

## Function Calling (`src/function.rs`)

Tools are external scripts/binaries:
- Declarations live in `functions.json` (JSON schema format)
- Binaries live in `functions/bin/`
- Agents can have their own function directories
- When an LLM returns tool calls, `eval_tool_calls()` invokes the corresponding binary with the arguments as JSON, reads output from a temp file (`$LLM_OUTPUT`)

Tool calls are deduplicated and infinite loops are detected.

---

## REPL (`src/repl/`)

Built on `reedline` with:
- 36 dot-commands (`.role`, `.session`, `.agent`, `.rag`, `.macro`, `.set`, `.file`, etc.)
- Tab completion with fuzzy matching
- Syntax highlighting
- Vi and Emacs keybinding modes
- Multi-line input (`::: ... :::`)
- Buffer editing with external editor (Ctrl+O)

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
3. **Recursive tool call resolution** — `start_directive` and `ask` are `#[async_recursion]`
4. **Trait-based polymorphism** — `Client` trait for providers, `RoleLike` trait for prompt sources
5. **Static lazy initialization** — `LazyLock` and `OnceLock` for expensive one-time computations
