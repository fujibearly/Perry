# Session Summary 19: Link-Only Web Search (`--links`), Sub-Agent Multi-Step Research Pipeline & Empty-Response Resilience

**Date:** 2026-09-09  
**Branch:** `feat/tool-safety-permission-boundary`  
**Test Suite Status:** 498 tests passing (490 unit/integration + 5 catalog override + 3 web assets, 0 failed); clippy clean; release binary built.  
**Focus Repos:** `/home/istari/projects/aichat` & `/home/istari/projects/llm-functions`

---

## 1. Executive Summary

In Session 19, we resolve the "three-tier echo chamber" where the `researcher` sub-agent echoed `web_search` output rather than inspecting primary sources with `fetch_url_via_curl.sh`. We implement an optional `--links` flag on `web_search_aichat.sh` to turn grounded search into an actionable URL provider, update agent instructions, and harden the core agent loop against transient 0-token empty responses from LLM providers.

### Key Deliverables:

1. **Link-Only Grounded Search (`web_search --links`):**
   - Added `# @flag --links` annotation and parameter schema in `tools/web_search_aichat.sh`.
   - Injected prompt framing instructing Gemini Google Search grounding to return strictly formatted `[Page Title](URL) - 1-sentence summary` entries rather than synthesizing an essay.
   - Verified that Google's Vertex AI Search grounding redirect URLs (`https://vertexaisearch.cloud.google.com/grounding-api-redirect/...`) return HTTP 302 redirects seamlessly followed by `curl -fsSL` into `html-to-markdown`.
   - Regenerated tool declarations in `agents/researcher/functions.json` via `argc build-declarations@agent researcher`.

2. **Sub-Agent Multi-Step Research Pipeline:**
   - Updated `agents/researcher/index.yaml` instructions to use `links=true` on its discovery pass, followed by 2–4 parallel fetches via `fetch_url_via_curl`.
   - Broke the 3-tier echoing cycle: `orchestrator` delegates $\rightarrow$ `researcher` searches with links $\rightarrow$ `fetch_url_via_curl` fetches and markdownifies target pages $\rightarrow$ `researcher` synthesizes real source content $\rightarrow$ `orchestrator` reports final findings.

3. **Empty LLM Response Detection & Automatic Retry (`call_llm_raw`):**
   - Root cause analysis: diagnosed an issue where a transient provider blip yielded 0 tokens in 0.9s; `agent_loop.rs` checked `tool_calls.is_empty()`, erroneously assumed the agent was done, emitted `LoopComplete`, and returned `""` to the orchestrator.
   - Added automatic exponential backoff retry (up to 2 retries, 500ms * attempt) in `src/agent_loop.rs` inside `call_llm_raw`.
   - Bails with an explicit error (`bail!("LLM returned an empty response with no text and no tool calls")`) if all attempts return empty, preventing silent success on 0-token outputs and alerting the parent orchestrator.

4. **Vertex AI / Gemini Streaming Error Catching (`src/client/vertexai.rs`):**
   - Extended `gemini_chat_events` to check `data["promptFeedback"]["blockReason"]` (`bail!("Blocked by provider: {block_reason}")`), `SAFETY`, and `RECITATION` finish reasons, preventing silent stream closures on filtered content.

5. **Demo Runner Stepping Order (`scripts/run-demos.nu`):**
   - Ensured demo header, natural-language description, and command line render **before** the interactive keypress pause in all 22 demos.

---

## 2. Technical Architecture & Data Flow

### Multi-Step Sub-Agent Pipeline
```
[User Request]
       │
       ▼
[Orchestrator Turn 1] ───> Delegates task to 'researcher'
                                │
                                ▼
                   [Researcher Turn 1]
                   Calls: web_search(query=..., links=true)
                                │
                                ▼ Returns candidate URLs
                   [Researcher Turn 2 & 3]
                   Calls: fetch_url_via_curl(url_1), fetch_url_via_curl(url_2) [Parallel]
                                │
                                ▼ Returns converted Markdown pages
                   [Researcher Turn 4]
                   Synthesizes findings + cites source URLs
                                │
                                ▼ Returns research report
[Orchestrator Turn 2] ───> Presents final synthesized response
```

### Empty-Response Resilience Flow
```
call_llm_raw()
       │
       ▼
Execute LLM request (streaming or non-streaming)
       │
       ├──> Non-empty text or tool_calls? ──> Return Ok((output, tool_calls))
       │
       └──> Empty text AND empty tool_calls?
                   │
                   ├──> retries < MAX_EMPTY_RETRIES?
                   │         │
                   │         └──> Increment retries, sleep(500ms * retries), loop
                   │
                   └──> retries exhausted?
                             │
                             └──> bail!("LLM returned an empty response with no text and no tool calls")
```

---

## 3. Verification & Live Trace

- **Unit & Integration Tests:** `cargo test` $\rightarrow$ **490 passed; 0 failed**.
- **Release Build:** `cargo build --release` completed cleanly.
- **Live Demo 4 Execution:**
  ```text
  Agent orchestrator (5244 (QuietOtter)) loop trace:
    [5244 (QuietOtter) [turn 1/20] starting]
    [5244 (QuietOtter) calling: researcher]
    [child researcher] TurnStart { turn: 1, max_turns: 20 }
  │   Agent researcher (5251 (DaringWolf)) loop trace:
  │     [5251 (DaringWolf) [turn 1/20] starting]
  │     [5251 (DaringWolf) calling: web_search]
  │     [5251 (DaringWolf) ALLOW web_search: risk safe <= ceiling safe]
    [child researcher] ToolStart { name: "web_search", id: None }
    [child researcher] SafetyGatePassed { name: "web_search", comparison: "risk safe <= ceiling safe" }
    [child researcher] ToolComplete { name: "web_search", duration: 6.977833404s, success: true }
    [child researcher] TurnStart { turn: 2, max_turns: 20 }
  │     [5251 (DaringWolf) web_search completed (7.0s)]
  │     [5251 (DaringWolf) [turn 2/20] starting]
  │     [5251 (DaringWolf) calling: fetch_url_via_curl]
    [child researcher] ToolStart { name: "fetch_url_via_curl", id: None }
    [child researcher] SafetyGatePassed { name: "fetch_url_via_curl", comparison: "risk safe <= ceiling safe" }
    [child researcher] ToolComplete { name: "fetch_url_via_curl", duration: 328.745103ms, success: true }
    [child researcher] TurnStart { turn: 3, max_turns: 20 }
  │     [5251 (DaringWolf) fetch_url_via_curl completed (0.3s)]
  │     [5251 (DaringWolf) [turn 3/20] starting]
  │     [5251 (DaringWolf) calling: fetch_url_via_curl]
  │     [5251 (DaringWolf) calling: fetch_url_via_curl]
    [child researcher] ToolComplete { name: "fetch_url_via_curl", duration: 243.34685ms, success: true }
    [child researcher] ToolComplete { name: "fetch_url_via_curl", duration: 456.253402ms, success: true }
    [child researcher] TurnStart { turn: 4, max_turns: 20 }
  │     [5251 (DaringWolf) [turn 4/20] starting]
  │     [5251 (DaringWolf) done]
    [child researcher] LoopComplete
    [5244 (QuietOtter) researcher completed (25.4s)]
    [5244 (QuietOtter) [turn 2/20] starting]
    [5244 (QuietOtter) done]
  ```
