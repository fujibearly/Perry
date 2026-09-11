# Session Summary 22: Web-Search Grounding Control (`--wslinks`), Streamlined Trace Display, Dual-Layer `MALFORMED_FUNCTION_CALL` Recovery & Truthful Failure Reporting

**Date:** 2026-09-11  
**Branch:** `feat/tool-safety-permission-boundary` (aichat) & `feat/fetch-url-native-html-to-markdown` (llm-functions)  
**Test Suite Status:** All 505 unit tests passing (`cargo test --bin aichat`, 0 failed); clippy clean (`-D warnings`); release binary compiled; 71/71 `argc test` passing in `llm-functions`.  
**Specifications & Artifacts:**
- [walkthrough-token-formatting-and-empty-response-diagnosis.md](file:///home/istari/.gemini/antigravity-cli/brain/d6bb2a10-57b0-4f59-8b7f-0b2f3fa74e9e/walkthrough-token-formatting-and-empty-response-diagnosis.md)
- [token-formatting-and-empty-response-diagnosis-plan.md](file:///home/istari/.gemini/antigravity-cli/brain/d6bb2a10-57b0-4f59-8b7f-0b2f3fa74e9e/token-formatting-and-empty-response-diagnosis-plan.md)
- [html-to-markdown-redirect-and-flags-analysis.md](file:///home/istari/.gemini/antigravity-cli/brain/d6bb2a10-57b0-4f59-8b7f-0b2f3fa74e9e/html-to-markdown-redirect-and-flags-analysis.md)
- [walkthrough-wslinks-and-trace-cleanup.md](file:///home/istari/.gemini/antigravity-cli/brain/d6bb2a10-57b0-4f59-8b7f-0b2f3fa74e9e/walkthrough-wslinks-and-trace-cleanup.md)

---

## 1. Executive Summary

In Session 22, we address orchestrator swarm web-search style flexibility, trace visual clutter, and parallel sub-agent resilience.

First, we implement **Web-Search Grounding Control (`--wslinks`)**, allowing users and workflows to choose between fast direct grounded web search (1-turn synthesis, no intermediate page scraping) and link exploration mode (`--wslinks`, multi-step URL discovery and secondary page scraping).

Second, we **streamline trace display** by omitting redundant initial agent and nanoworker "loop trace" header lines, conserving vertical terminal space while preserving indentation, petnames, color coding, and turn tracking.

Third, we resolve parallel sub-agent crashes in Demo 5 caused by Gemini 2.5 Flash hallucinating Python function syntax under parallel burst load (`MALFORMED_FUNCTION_CALL`), implementing a **dual-layer recovery system** (system prompt guidance + AST/kwargs parser in `src/client/vertexai.rs`) and automatic transient retries.

Fourth, we fix **trace masking** in `src/agent_loop.rs` where child process failures returning error JSON were falsely reported as `completed` in green, restoring truthful `FAILED` reporting in soft coral red.

Finally, we format URL tokens on dedicated lines in `llm-functions` to eliminate base64 token corruption and verify live Demo 5 in both `--wslinks` and direct grounded modes.

---

## 2. Key Deliverables & Architecture Changes

### A. Web-Search Style & Branch-Wide Grounding Control (`--wslinks` & `AICHAT_WSLINKS`)
- **CLI Flag & Environment Propagation:**
  - Added `--wslinks` flag to `src/cli.rs`. When specified, exports `AICHAT_WSLINKS=true` into the environment.
  - Subprocess delegation in `eval_agent_tool_subprocess` (`src/agent_loop.rs`) automatically passes `--wslinks` and `AICHAT_WSLINKS=true` down the agent hierarchy.
- **Dynamic Instruction Interpolation:**
  - Added `{{__researcher_search_instructions__}}` variable in `src/utils/variables.rs` and `src/config/agent.rs`.
  - **Default (no `--wslinks`):** Directs the `researcher` agent to use grounded `web_search` (`links: false`) and synthesize directly without fetching individual web pages.
  - **Active (`--wslinks`):** Directs the `researcher` agent to discover links (`links: true`) and fetch 2–4 pages with `fetch_and_summarize`.
- **Tool Guard:**
  - `tools/web_search_aichat.sh` enforces that `--links` formatting is active only when `AICHAT_WSLINKS=true`.
- **Demo Runner Integration:**
  - Updated `scripts/run-demos.nu` to accept `--wslinks`, setting or clearing `AICHAT_WSLINKS` and forwarding `--wslinks` to demos (Demo 4, Demo 5, Demo 11).

### B. Streamlined Trace Display
- Omitted redundant initial agent/nanoworker loop trace headers (`Agent <name> loop trace:`) in `src/agent_loop.rs`.
- Preserved visual hierarchy: 4-space indentation per depth, ancestor vertical guide rails (`│     `), disposable British humor petnames (`12345 (FickleCousin)`), 11-color ANSI palettes, and clear turn tracking (`[turn 1/20] starting`).
- Ensured atomic line writes via `write_atomic_terminal_output` to prevent multi-process line collisions on `/dev/tty`.

### C. Streaming & Candidate Error Bubble-Up (`src/client/vertexai.rs`)
- Bubbled non-`STOP` `finishReason` when candidates lack content parts, preventing silent drops or confusing empty responses when the model triggers safety filters or recitation blocks.

### D. Dual-Layer `MALFORMED_FUNCTION_CALL` Recovery & Transient Retries
- **Root Cause:** Under parallel burst calls, Gemini 2.5 Flash intermittently emitted Python syntax (e.g. `print(default_api.web_search(query="...", links=true))`) instead of JSON function call objects. Google's API rejected this with `finishReason: "MALFORMED_FUNCTION_CALL"`, aborting sub-agents within ~1.1s.
- **Prompt Layer:** Added explicit system prompt instructions in `gemini_build_chat_completions_body`:
  `When invoking tools, call the tool directly using the exact declared function name. Do NOT output code, python calls, or prepend namespaces (e.g. do NOT write print(default_api....)).`
- **AST / Kwargs Recovery:** Added `recover_malformed_function_call` and `parse_python_kwargs` in `src/client/vertexai.rs` to extract tool names and reconstruct valid JSON argument maps from `finishMessage` kwargs for both streaming and non-streaming modes.
- **Transient Retries:** Added exponential backoff retries in `call_llm_raw` (`src/agent_loop.rs`) for transient provider errors (`MALFORMED_FUNCTION_CALL`, `ResourceExhausted`, `429`, `503`).

### E. Truthful Tool Failure Trace Reporting (`src/agent_loop.rs`)
- In `eval_tool_calls_parallel`, inspected `let is_error = value.is_object() && value.get("error").is_some();`.
- Emitted `AgentLoopEvent::ToolComplete { success: !is_error, ... }`.
- Sub-agent exits with non-zero codes now truthfully display `FAILED` in soft coral red (`#e06c75`) rather than false green `completed`.

### F. URL Token Formatting & Strict JSON Constraints (`llm-functions`)
- Updated `tools/web_search_aichat.sh` to output canonical URLs on dedicated lines:
  ```text
  Title: <Page Title>
  URL: <Full Canonical URL>
  Summary: <1-sentence summary>
  ```
  Eliminated markdown link brackets (`[title](url)`) that previously corrupted long base64 tokens and caused 404 redirect failures in downstream URL fetching.
- Added strict JSON validation constraints in `agents/researcher/index.yaml` requiring all tool call arguments to be strictly valid JSON with properly escaped strings.

---

## 3. Verification Evidence

### 1. Isolated Parallel Sub-Agent Test
Concurrent execution of two researcher agents in parallel processes:
- PID 1 (Rust async runtimes 2025 comparison): exit code 0, 6,886 input + 739 output tokens.
- PID 2 (Python asyncio vs trio comparison): exit code 0, 10,845 input + 1,633 output tokens.
- Both processes returned full research syntheses cleanly.

### 2. Live Demo 5 Execution (`--wslinks` Mode)
Command: `nu scripts/run-demos.nu --demo 5 --wslinks`
- **+2.4s:** Orchestrator calls two `researcher` sub-agents concurrently.
- **+2.6s - +2.7s:** `WaryPeer` (Rust) and `NarkyCad` (Python) start in parallel.
- **+3.6s:** Both sub-agents call `web_search` concurrently without crashes.
- **+13.8s - +15.2s:** Nanoworkers complete search successfully.
- **+18.3s - +28.3s:** Both sub-agents scrape pages via `fetch_and_summarize` and `summarize_text`.
- **+32.6s:** `NarkyCad` completes (30.2s).
- **+35.3s:** `WaryPeer` completes (33.0s).
- **+40.6s:** Orchestrator synthesizes both comparisons cleanly.
- **Result:** Both sub-agents completed successfully, 0 crashes, 0 orchestrator retries.

### 3. Live Demo 5 Execution (Direct Grounded Mode)
Command: `nu scripts/run-demos.nu --demo 5`
- Orchestrator launched two parallel researchers with default `links: false`.
- Sub-agents returned comprehensive grounded search syntheses directly.
- Orchestrator synthesized comparisons in 33.9s without secondary page scrapes.

### 4. Automated Regression Tests
- Unit test `test_recover_malformed_function_call` in `src/client/vertexai.rs` passed.
- All 505 unit tests in `aichat` passed (`cargo test --bin aichat -- --test-threads=1`).
- `cargo clippy -- -D warnings` passed with 0 warnings.
- `argc test` in `llm-functions` passed 71/71 tests (100%).

---

## 4. Worktree State & Git Alignment

- **`aichat`** (`feat/tool-safety-permission-boundary`): clean working tree.
- **`llm-functions`** (`feat/fetch-url-native-html-to-markdown`): clean working tree.
