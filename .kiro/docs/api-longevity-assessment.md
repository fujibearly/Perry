# Provider API Longevity Assessment

**Date:** August 2026

## Summary

All 8 provider client implementations in the fork are functional today. No immediate breakage. The main risks are Google's Interactions API replacing `generateContent` (no shutdown date yet) and Azure's shift to a v1 URL pattern.

## Ratings

| Provider | Rating | Urgency | Action Required |
|----------|--------|---------|-----------------|
| OpenAI | Excellent | None | None — already implements Responses too |
| Claude | Excellent | None | None |
| Gemini | Moderate | Low | Plan Interactions API migration (no deadline yet) |
| Azure OpenAI | Moderate | Low | URL pattern update to v1 path (trivial) |
| Bedrock | Excellent | None | None |
| Cohere | Good | None | Keep models.yaml embedding models current |
| Vertex AI | Moderate | Low | Same as Gemini + monitor platform rebranding |
| OpenAI-Compatible | Excellent | None | models.yaml maintenance only |

## Per-Provider Details

### 1. OpenAI (`openai.rs`)

- **Endpoint:** `/v1/chat/completions` — still fully supported, no deprecation announced
- **Auth:** Bearer token — unchanged
- **Status:** OpenAI recommends Responses API for new projects, but Chat Completions is not deprecated
- **Fork bonus:** Already implements `/v1/responses` for multi-agent
- **Longevity:** 18+ months. The format is an industry standard cloned by dozens of providers.

### 2. Claude/Anthropic (`claude.rs`)

- **Endpoint:** `/v1/messages` — current and actively developed
- **Version header:** `anthropic-version: 2023-06-01` — still current in 2026
- **Status:** Anthropic maintains backward compat. Additions come via opt-in beta headers only. "Code written in 2024 still works in 2026."
- **Longevity:** 18+ months. No successor announced.

### 3. Gemini (`gemini.rs`)

- **Endpoint:** `v1beta/models/{model}:generateContent` — now labelled "legacy" since June 2026
- **New standard:** Interactions API is GA and recommended for all new projects
- **Status:** `generateContent` "remains fully supported" — no shutdown date announced
- **Model risk:** Gemini 2.5 models shutting down Oct 2026 (models.yaml update only)
- **Longevity:** 12–18 months. Will eventually need a rewrite to the Interactions API wire format.

### 4. Azure OpenAI (`azure_openai.rs`)

- **Endpoint:** `/openai/deployments/{model}/chat/completions?api-version=2024-12-01-preview`
- **New standard:** v1 API (Aug 2025) uses `/openai/v1/chat/completions`, no `api-version` param
- **Status:** Old dated versions still function. No retirement date announced for `2024-12-01-preview`.
- **Workaround:** Users can set `api_base` to include `/openai/v1` in config today.
- **Longevity:** 12–18 months. Migration is mostly a URL change.

### 5. AWS Bedrock (`bedrock.rs`)

- **Endpoint:** `bedrock-runtime.{region}.amazonaws.com/model/{model}/converse`
- **Protocol:** Converse API with binary event-stream
- **Auth:** SigV4 signing — standard AWS
- **Status:** Actively invested (batch inference Converse support added Feb 2026). No deprecation signals.
- **Longevity:** 18+ months. AWS's strategic unified inference API.

### 6. Cohere (`cohere.rs`)

- **Endpoint:** `api.cohere.ai/v2/chat` — current production API
- **Status:** v2 Chat API actively developed (North Mini Code, Transcribe Arabic). Embed v2.0 retired April 2026.
- **Longevity:** 18+ months. Wire protocol is current; only model names need updates.

### 7. Vertex AI (`vertexai.rs`)

- **Endpoint:** `{location}-aiplatform.googleapis.com/v1/publishers/{publisher}/models/{model}:{action}`
- **Platform:** Vertex AI rebranded to "Gemini Enterprise Agent Platform"
- **Status:** REST endpoints unchanged. `generateContent` has same "legacy" status as direct Gemini. Extensions deprecated Nov 2026.
- **Claude on Vertex:** `anthropic_version: vertex-2023-10-16` — still current
- **Longevity:** 12–18 months. Same Interactions API pressure as Gemini; platform rebranding could eventually change URL patterns.

### 8. OpenAI-Compatible (`openai_compatible.rs`)

- **Wire format:** OpenAI `/chat/completions` — the de facto industry standard
- **Catalog:** ~20 providers (Mistral, DeepSeek, xAI, Perplexity, Groq, Cloudflare, Qianwen, etc.)
- **Status:** These providers exist because they're OpenAI-compatible. Breaking compat loses them customers.
- **Longevity:** 24+ months. The protocol is too entrenched to die.

## Key Takeaways

1. Nothing is broken today. All clients work against current provider APIs.
2. The only real migration pressure is from Google (Interactions API replacing generateContent). No shutdown date yet.
3. Azure's v1 API is a nice-to-have modernization — old pattern still functions.
4. The biggest ongoing cost is `models.yaml` maintenance (model names, pricing, capabilities).
5. The multi-wire-format architecture (`WireFormat` enum) means ~20 providers work without new client code.
