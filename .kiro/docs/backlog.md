# aichat Fork Backlog

## 1. Rust MCP Bridge ✓

**Status:** Complete — committed on `feat/rust-mcp-bridge` (commit `3e95825`)  
**Scope:** 1272 lines added (9 files)  
**Spec:** `.kiro/specs/rust-mcp-bridge/` (requirements, design, tasks)

Replaced the Node.js MCP bridge with an in-process Rust implementation. MCP-sourced tools appear identical to shell-exec tools — no new abstractions, two integration points (`ToolCall::eval()` routing + `Config::load_functions()` discovery). Cached tool manifests for fast startup, lazy server spawn on first invocation, graceful shutdown on exit. Behind `mcp` cargo feature flag (default on).

---

## 2. Gemini Interactions API Module

**Priority:** High  
**Scope:** ~1000-1500 lines (new file: `src/client/gemini_interactions.rs`)  
**Driver:** Google's `generateContent` endpoint is labelled "legacy" since June 2026. The Interactions API is GA, supports both Gemini 2.x and 3.x models uniformly, and maps almost 1:1 to the pattern already established in `openai_responses.rs`.

### Approach

Create a self-contained module following the `openai_responses.rs` template. This does NOT replace `gemini.rs` — both coexist, with routing based on model capability or user preference.

**Architecture:**

```
gemini.rs            → generateContent (legacy, still works, kept for older models)
gemini_interactions.rs → Interactions API (new, agentic, streaming)
vertexai.rs          → Routes to either based on model/config
```

**What to implement:**

1. **Session lifecycle** — `POST /interactions` to create, response contains typed `steps[]` array.
2. **Step types** — Map `thought`, `model_output`, `function_call`, `function_result` to existing `ChatEvent` variants (`Reasoning`, `Text`, `ToolUse`, `ToolResult`).
3. **Tool-use loop** — When status is `requires_action`, execute tool calls locally, send results back, continue. Same pattern as `openai_responses.rs` multi-turn loop.
4. **Streaming** — SSE with `step.start`, `step.delta` (text, thought_summary, thought_signature, arguments_delta), `step.stop`. Map deltas to `SseEvent::Text` / `SseEvent::Reasoning`.
5. **Built-in tools** — Support `google_search`, `code_execution` as pass-through (server-side execution, client just receives results).
6. **Thinking** — Parse `thought` steps and `thought_signature` for multi-turn reasoning continuity.
7. **Auth** — Reuse existing Gemini API key auth and VertexAI OAuth token flow.

**What this solves:**

- Eliminates the Gemini 2.x vs 3.x branching problem (both use same endpoint/response format)
- Gives Gemini parity with OpenAI's agentic capabilities in the fork
- Future-proofs against `generateContent` deprecation
- Enables native tool-use loops for Gemini without server-side orchestration dependency

**Reference materials:**

- Interactions API spec: `https://ai.google.dev/api/interactions`
- Existing template: `src/client/openai_responses.rs`
- Current legacy impl: `src/client/gemini.rs`, `src/client/vertexai.rs`

---

## 3. Client-Side Agent Loop Enhancements

**Priority:** High  
**Scope:** ~300-500 lines (modifications to `src/main.rs`, `src/function.rs`, possibly new `src/orchestrate.rs`)  
**Driver:** The classic agent loop (`run_directive`) is the only provider-agnostic orchestration in aichat. It works with Claude, Gemini, Cohere — anything that returns tool_calls. But it's too primitive for multi-step agentic work. Enhancing it gives every provider agentic capabilities without requiring server-side orchestration support.

### Approach

Enhance the existing client-side recursive loop *without* merging it with the server-side multi-agent mode. They coexist — different delegation models, shared tool execution layer.

**Enhancements (ordered by dependency):**

1. **Parallel tool execution** — When the LLM returns multiple tool_calls in one response, execute them concurrently (tokio::spawn or join_all). The `ToolCall.id` field already supports correlation. Fall back to serial for tools marked as having side-effects.

2. **Max-turns budget** — Add a configurable `max_turns` (default: 20?) to prevent runaway recursion. Currently the loop recurses unbounded until the LLM stops emitting tool_calls. Surface a warning when budget is exhausted.

3. **Agent-as-tool (sub-agent delegation)** — The `FunctionDeclaration` already has an `agent: bool` field. When a tool call targets an agent-flagged function, instead of shelling out, spawn a sub-agent session in-process: construct a new `Input` with the sub-agent's instructions/tools, run the recursive loop, return the output as the tool result. This enables multi-agent composition for *any* provider.

4. **Progress/trace reporting** — Generalize the `OpenAIResponsesProgress` live trace to work with the classic loop too. Emit events like "calling tool X", "turn 3/20", "sub-agent Y started". Same spinner/trace infrastructure.

5. **Optional planning tool** — A built-in pseudo-tool (`_plan` or `_think`) that the LLM can call to write to a scratchpad. The content is appended to the next turn's context but not shown to the user. Gives the model a way to decompose tasks without polluting output.

**What this does NOT do:**

- Does not merge with server-side multi-agent mode
- Does not add memory/persistence across sessions
- Does not change how tools are defined or discovered (that's backlog #1)
- Does not add human-in-the-loop pauses (future consideration)

**Key principle:** The server-side loop (OpenAI Responses, future Gemini Interactions) and the client-side loop are peers. Server-side is better when the provider supports it (lower latency, provider-managed state). Client-side is the universal fallback that makes every provider agentic.

---

## Dependency Graph and Sequencing

The three backlog items are coupled. Here's the dependency structure:

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
  │ (new backend   │  │ Loop Enhance    │  │ Interactions        │
  │  for tool exec)│  │ (orchestration) │  │ (server-side loop)  │
  └───────┬────────┘  └───────┬─────────┘  └──────────┬──────────┘
          │                    │                        │
          │              depends on #1                  │
          │              (MCP tools need               │
          │               to be callable)              │
          │                    │                        │
          └────────────────────┼────────────────────────┘
                               │
                    ┌──────────▼──────────┐
                    │  All three feed the  │
                    │  same FunctionDecl   │
                    │  + ToolResult model  │
                    └─────────────────────┘
```

**Sequencing:**

| Phase | Item | Rationale |
|-------|------|-----------|
| **Phase 1** | #1 Native MCP | Foundation. Unblocks tool ecosystem access for both loops. Self-contained, testable in isolation. No changes to orchestration logic. |
| **Phase 2** | #3 Client-Side Enhancements | Builds on #1 (MCP tools are now callable in parallel, as sub-agent tools, etc.). Makes the classic loop competitive. Still provider-agnostic. |
| **Phase 3** | #4 Tool Output Routing | Builds on #3 (integrates into the async dispatch layer). Makes tool composition practical. |
| **Phase 4** | #2 Gemini Interactions | Most complex, most external dependency (API stability). By this point the tool execution layer is solid and the pattern is proven by OpenAI Responses. Can reference both existing server-side code AND enhanced client-side code for design decisions. Alternatively, skip entirely if OpenRouter coverage is sufficient. |

**Why this order:**

- #1 is pure infrastructure with no architectural risk. It adds a capability without changing any existing behavior.
- #3 is the highest-leverage improvement for daily use — it makes *every* provider better, not just one. And it needs #1 done so MCP tools participate in parallel execution and sub-agent delegation.
- #2 is the longest-term bet. Google's API may still shift. Having #1 and #3 done first means the Gemini module can be written against a mature tool execution layer, and if the Interactions API changes, the classic loop is already strong enough to cover Gemini via `generateContent` + client-side orchestration.

---

## Future Considerations (not yet scoped)

- **OpenAPI ingestion** — Parse OpenAPI specs to auto-generate tool definitions, invoke via HTTP. Third backend for ToolDispatch.
- **Structured output abstraction** — Uniform JSON Schema enforcement across providers (all now support it, but aichat doesn't abstract it).
- **Context/memory management** — Sliding window, summarization, or vector-backed recall for long sessions.
- **Human-in-the-loop** — Configurable pause points in the agent loop (after N turns, before destructive tools, on sub-agent delegation).

---

## 4. Tool Output Routing

**Priority:** Medium  
**Scope:** ~200 lines (modifications to `src/function.rs`, `src/agent_loop.rs`, `src/config/mod.rs`)  
**Driver:** Today, every tool result goes back into the LLM's conversation context. For tools that produce large or final outputs, this is wasteful or wrong — a generated report shouldn't be re-digested by the model, it should go to a file. Data fetched for a pipeline step should go to the next tool, not through the LLM. Routing control makes tool composition practical without burning context.

### Approach

Allow tools to declare where their output goes, instead of always returning it to the conversation context. Three destinations:

1. **context** (default, current behavior) — result goes into the next LLM turn as a tool_result message.
2. **file** — result is written to a specified path. The model receives a confirmation (`"written to /tmp/report.md, 2.4 KB"`) instead of the full content.
3. **pipe** — result is passed as input to another named tool. The model receives the final piped tool's result. Enables tool chains without round-tripping through the LLM.

**Configuration:**

```yaml
# In functions.json or agent config
[
  {
    "name": "generate_report",
    "description": "Generate a markdown report",
    "parameters": { ... },
    "output": { "destination": "file", "path": "/tmp/{{name}}.md" }
  },
  {
    "name": "fetch_raw_data",
    "description": "Fetch raw dataset",
    "parameters": { ... },
    "output": { "destination": "pipe", "target": "summarize_data" }
  }
]
```

Alternatively, the LLM could specify routing at call time via a special `_output` argument, making it dynamic rather than static declaration.

**What this solves:**

- Prevents context window pollution from large intermediate results
- Enables tool pipelines (fetch → transform → summarize) without LLM round-trips per step
- Makes "write a file" tools idiomatic (tool output IS the file, not text the model has to then write)
- Reduces token cost for workflows that produce artifacts

**What this does NOT do:**

- Does not change the tool discovery/definition format (augments it with an optional field)
- Does not introduce streaming between tools (pipe is batch: tool A finishes, then tool B gets the full result)
- Does not affect the agent loop's turn counting (a piped chain counts as one tool execution)

**Dependencies:** Backlog #3 (agent loop enhancements) should be complete first — routing integrates into the async parallel dispatch layer.
