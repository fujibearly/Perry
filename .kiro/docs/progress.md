# Project Progress

## Current State (2026-08-21)

**Branch:** `feat/rust-mcp-bridge` (off `main`)  
**Version:** v0.31.0-fork.9  
**Last commit:** `3e95825` — feat: add native Rust MCP bridge

## What's Done

### Backlog #1: Rust MCP Bridge ✓

Full implementation committed. 1272 lines, 298 tests passing (full suite), zero warnings.

Key decisions made during implementation:
- **Option C** — MCP tools look identical to shell-exec from the rest of the codebase. No new dispatch trait, no enum of backends. Two insertion points only.
- **Cached manifests** — Tool schemas cached to disk (`<config-dir>/mcp-cache/<server>.json`), invalidated by config hash. `--sync-mcp` for forced refresh.
- **block_in_place bridge** — `eval_tool_calls` is synchronous today; MCP module is internally async; bridged via `tokio::task::block_in_place`. When backlog #3 makes eval_tool_calls async, the bridge becomes a direct await.
- **Naming:** `server__tool` (double underscore) for namespaced tool names.
- **Feature flag:** `mcp` (default on), compile out with `--no-default-features`.

Files touched:
- `Cargo.toml` — feature flag, tokio process+io-util features
- `src/mcp.rs` — NEW, the entire bridge module
- `src/main.rs` — mod, --sync-mcp handler, shutdown hook
- `src/cli.rs` — --sync-mcp flag
- `src/function.rs` — Functions::extend(), MCP routing in ToolCall::eval()
- `src/config/mod.rs` — mcp_servers field, mcp_tools field, load in load_functions(), use_tools filtering, info display
- `src/config/agent.rs` — mcp_servers in AgentConfig, merge+load in Agent::init()
- `config.example.yaml` — MCP servers example section
- `config.agent.example.yaml` — Agent-level MCP servers example

Spec at: `.kiro/specs/rust-mcp-bridge/` (requirements.md, design.md, tasks.md)

## What's Next

### Backlog #3: Client-Side Agent Loop Enhancements (Phase 2)

The next item per the sequencing plan. Key enhancements:
1. Parallel tool execution (tokio join_all for independent tool_calls)
2. Max-turns budget (configurable, prevent runaway recursion)
3. Agent-as-tool (sub-agent delegation via the `agent: bool` field)
4. Progress/trace reporting (generalize OpenAIResponsesProgress for classic loop)
5. Optional planning tool (scratchpad for LLM task decomposition)

This makes every provider agentic without server-side orchestration. Depends on #1 being done (MCP tools participate in parallel execution).

### Backlog #2: Gemini Interactions API (Phase 3)

Longest-term item. New `src/client/gemini_interactions.rs` following `openai_responses.rs` template. Wait for API stability confirmation before starting.

## Architecture Decisions Log

| Decision | Rationale |
|----------|-----------|
| Don't merge server-side and client-side agent loops | They're different delegation models (who decides what to call). Shared tool execution layer, separate orchestration. |
| MCP as bridge, not native client | It IS a bridge — replacing Node.js with Rust. Honest naming: `rust-mcp-bridge`. |
| Option C (transparent integration) | Follow sigoden philosophy: tools are tools, minimal Rust surface, no new abstractions. |
| Cached manifests with --sync-mcp | Daily driver optimization. Don't spawn servers on every startup. |
| Feature flag | Allows compile-out for minimal builds. |

## Branch Status

- `main` — upstream fork at v0.31.0-fork.9
- `feat/rust-mcp-bridge` — MCP implementation (ready to merge to main when tested)

## Environment Reminders

- Production aichat: `/usr/bin/aichat` (v0.30.0), config at `~/.config/aichat/`
- Dev binary: `~/projects/aichat/target/release/aichat`
- To test without conflicting: `AICHAT_CONFIG_DIR=/tmp/aichat-test` (or just use same config — it's read-only)
- Live functions (don't touch): `~/clones/llm-functions`
- Dev functions (safe): `~/projects/llm-functions`
