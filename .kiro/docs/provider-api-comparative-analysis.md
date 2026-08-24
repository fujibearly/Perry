# Provider API Comparative Analysis: OpenAI, Anthropic, Gemini

**Date:** August 2026

## Overview

This document compares the three major provider implementations in the aichat fork — their current API approaches, how model evolution has impacted wire formats, and where each stands in terms of longevity and required maintenance.

The key finding: all three providers are converging on the same architectural pattern (server-side stateful orchestration with typed steps), but they're at different stages of the transition. The fork already implements the modern pattern for OpenAI, inherits long-term stability from Anthropic's design philosophy, and has a clear path forward for Gemini based on the existing OpenAI Responses precedent.

---

## The Three Implementations

### OpenAI (`openai.rs` + `openai_responses.rs`)

**Legacy path:** `/v1/chat/completions` — stateless request/response, client manages history.

**Modern path:** `/v1/responses` — server-side multi-agent orchestration with typed output items, tool pause/resume loops, streaming SSE structural events.

**Fork status:** Both paths implemented. The Responses module (`openai_responses.rs`) is a substantial ~1800-line implementation covering:
- Multi-turn tool execution loop (up to 64 continuation turns)
- Function call caching and deduplication
- Live progress tracking via `OpenAIResponsesProgress`
- Web search source extraction and citation rendering
- Per-turn cost calculation with service tier multipliers
- Sanitized trace output (never leaks content/arguments)

**Model generations:** GPT-5.x models work through both paths. Responses multi-agent requires GPT-5.6 specifically.

### Anthropic/Claude (`claude.rs`)

**Single path:** `/v1/messages` — stateless, additive-only evolution.

**Fork status:** Comprehensive implementation with:
- Full streaming state machine (`ClaudeStreamState` with typed content blocks)
- Extended thinking / adaptive thinking support (both streaming `thinking_delta` and non-streaming `thinking` blocks)
- Signature handling (consumed but not round-tripped)
- Tool use with streaming JSON accumulation
- Refusal detection (`stop_reason: "refusal"`, fallback blocks)
- Sanitized error handling

**Model generations:** Claude 3.x → 4.x → Opus 5 all use the same Messages API wire format. Thinking was added as a new content block type (`type: "thinking"`), not as a protocol change. The fork handles all generations with a single code path.

**Why it works:** Anthropic's versioning philosophy is additive-only. One version header (`2023-06-01`) for three years. New features arrive via opt-in beta headers. The fork's Claude code is solid because Anthropic designed their API not to break clients — the merit belongs to Anthropic's engineering, not to the fork's.

### Gemini (`gemini.rs` / `vertexai.rs`)

**Legacy path:** `v1beta/models/{model}:generateContent` — stateless, flat `parts[]` array response.

**Modern path:** `POST /v1beta/interactions` — server-side state, typed `steps[]` array, thinking as dedicated step type.

**Fork status:** Only the legacy `generateContent` path is implemented. The Interactions API has zero support today.

**Model generations:**
- Gemini 2.x: Simple chat Q&A. `generateContent` works perfectly. No thinking metadata.
- Gemini 3.x: Reasoning-first, agentic. `generateContent` still works but returns an opaque `thoughtSignature` bolted onto text parts — no dedicated thinking representation. Multi-turn reasoning continuity is broken without signature round-tripping.

**The gap:** The fork's Gemini parser only handles `part["text"]` and `part["functionCall"]`. It does not emit `ChatEvent::Reasoning` for thinking content (unlike Claude). It does not round-trip `thoughtSignature` for multi-turn continuity. It cannot leverage server-side state, background execution, or built-in tools (Google Search, code execution, MCP servers).

---

## The Convergence Pattern

All three providers have converged on the same architectural pattern, just with different naming:

| Concept | OpenAI | Anthropic | Gemini |
|---------|--------|-----------|--------|
| Legacy stateless API | Chat Completions | — (Messages IS the stable API) | generateContent |
| Modern stateful/agentic API | Responses API | Managed Agents (separate product) | Interactions API |
| Response structure | Typed `output[]` items | Typed `content[]` blocks | Typed `steps[]` array |
| Thinking surfaced as | Encrypted reasoning items | `thinking` content blocks | `thought` steps with signature + summary |
| Tool pause mechanism | Function calls in output, loop | `stop_reason: "tool_use"`, client loops | `status: "requires_action"`, function_call steps |
| Server state continuity | Conversations API | Not applicable (Messages is stateless by design) | `previous_interaction_id` |
| Built-in hosted tools | `web_search` tool type | Part of Managed Agents only | `google_search`, `code_execution`, `mcp_server`, `url_context` |
| Streaming protocol | SSE with structural events | SSE with content_block_start/delta/stop | SSE with step.start/step.delta/step.stop |

The critical difference: **Anthropic kept their existing API as the stable foundation** and built Managed Agents as a separate, additive product. OpenAI and Google both created replacement APIs (`Responses`, `Interactions`) that subsume and deprecate the legacy ones.

---

## The Gemini Gap and Why the Fork Is Ready

### What's missing today

1. **No Interactions API support** — the entire modern Gemini path is absent
2. **No thinking output for Gemini 3.x** — thoughts are silently discarded
3. **No `thoughtSignature` round-tripping** — multi-turn reasoning degrades
4. **No server-side state** — full history re-sent every turn
5. **No built-in tool access** — can't use Google Search, code execution, or server-side MCP

### Why the fork is well-positioned to fix it

The `openai_responses.rs` module establishes the exact architectural pattern needed:

| Responses module (exists) | Interactions module (needed) |
|---------------------------|------------------------------|
| `POST /v1/responses` | `POST /v1beta/interactions` |
| Consumes SSE stream into complete response | Consumes SSE stream into complete interaction |
| Parses typed output items | Parses typed steps array |
| Loops on function_call items | Loops on `status: requires_action` |
| Emits live progress/trace events | Same pattern for step streaming |
| Caches tool results to avoid re-execution | Same optimization applies |
| `OpenAIResponsesProgress` for observability | Equivalent `GeminiInteractionsProgress` |

The scope is bounded: ~1000–1500 lines of self-contained code, following a proven template. No architectural changes to the rest of aichat required — the module would produce a standard `ChatCompletionsOutput` at the end, just like `openai_responses.rs` does.

### The 2.x vs 3.x problem disappears

The Interactions API accepts both Gemini 2.x and 3.x models through the same endpoint and response format:
- 2.x models produce `model_output` steps only — equivalent to today's behavior
- 3.x models produce `thought` steps interleaved with `model_output` and `function_call` steps
- The parsing code is identical for both — you iterate `steps[]` and handle each type
- No branching on model generation required

This eliminates the current awkwardness where `generateContent` returns subtly different response shapes depending on model generation.

---

## Current Risk Assessment

| Provider | API Stability | Fork Completeness | Action Needed | Timeline |
|----------|--------------|-------------------|---------------|----------|
| OpenAI | Excellent — Chat Completions not deprecated, Responses already implemented | Complete for both paths | None | — |
| Anthropic | Excellent — Messages API unchanged since 2023, thinking fully supported | Complete | Minor: signature round-tripping for multi-turn quality | Low priority |
| Gemini | Moderate — generateContent labelled "legacy", Interactions is GA | Legacy path only | Build Interactions module | When 3.x becomes primary usage or generateContent gets a shutdown date |

---

## Recommendation

1. **Keep `generateContent` working** as-is for Gemini 2.x models until their Oct 2026 shutdown
2. **Build `gemini_interactions.rs`** when Gemini 3.x becomes the primary Gemini use case — the spec is stable (GA since June 2026, already past breaking changes), the template exists, and the scope is well-defined
3. **No changes needed** for OpenAI or Anthropic paths — both are healthy and complete
4. **The fork's architecture (self-contained per-provider modules) is validated** — each provider's agentic API gets its own module without disturbing the others
