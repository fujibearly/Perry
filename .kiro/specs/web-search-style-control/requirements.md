# Web-Search Style & Branch-Wide Grounding Control (`--wslinks`) — Requirements

## Summary

Provide branch-wide control over the web-search style across the entire orchestrator process tree via a new CLI parameter `--wslinks`.
- **Default behavior (absence of `--wslinks`)**: `web_search` does NOT use `--links` (`links: false` or omitted). The researcher agent performs direct grounded search via the search provider's grounded summary in one turn, eliminating the multi-turn URL discovery and secondary `fetch_and_summarize` (and `|summarize_text`) pipeline stages.
- **Link-exploration behavior (presence of `--wslinks`)**: Preserves the legacy multi-step research behavior (`web_search` with `links: true` to discover 3–5 canonical URLs, followed by 2–4 `fetch_and_summarize` calls piped to `summarize_text`).

## Context & Motivation

In current orchestration workflows, the `researcher` agent is instructed to always run `web_search` with `links: true`, extract links, and then sequentially or in parallel issue 2–4 `fetch_and_summarize` calls (each of which invokes `html-to-markdown` and pipes through `summarize_text`). 

While deep page fetching is useful when full documentation scraping is necessary, in many common research queries (e.g., comparing libraries, factual inquiries, quick overviews), the native search provider (such as Gemini with Google Search grounding) already synthesizes a high-quality, up-to-date grounded answer with citations directly in the first turn. Running 3–5 additional page scrapes burns tokens, takes 30–60 seconds, and risks 404/bot-block failures.

Introducing `--wslinks` makes fast, direct grounded search the default, while allowing operators and test harnesses to opt into deep link exploration for the entire agent tree with a single flag.

## Functional Requirements

### FR-1: `--wslinks` CLI Parameter & Process Tree Propagation
- **FR-1.1**: The `aichat` CLI MUST support a boolean flag `--wslinks` in `src/cli.rs`.
- **FR-1.2**: When `--wslinks` is supplied on the command line (or if `AICHAT_WSLINKS=true` is present in the parent environment), `aichat` MUST set and export `AICHAT_WSLINKS=true` into the process environment during startup.
- **FR-1.3**: When `--wslinks` is not supplied and `AICHAT_WSLINKS` is not set, `AICHAT_WSLINKS` MUST remain unset/false in the environment.

### FR-2: Subprocess Delegation Propagation
- **FR-2.1**: In `eval_agent_tool_subprocess` (`src/agent_loop.rs`), when spawning a child `aichat` subprocess (e.g. orchestrator delegating to `researcher` or `coder`), if `AICHAT_WSLINKS=true` is active:
  - It MUST append `--wslinks` to the child command arguments.
  - It MUST set the environment variable `cmd.env("AICHAT_WSLINKS", "true")`.
- **FR-2.2**: If `AICHAT_WSLINKS` is not active, `--wslinks` MUST NOT be passed to the child process.

### FR-3: Dynamic Instruction Template Variable (`{{__researcher_search_instructions__}}`)
- **FR-3.1**: `interpolated_instructions()` in `src/config/agent.rs` (supported by `src/utils/variables.rs`) MUST resolve the template variable `{{__researcher_search_instructions__}}`.
- **FR-3.2**: When `AICHAT_WSLINKS=true` is active, `{{__researcher_search_instructions__}}` MUST resolve to the link-exploration instructions:
  ```text
  1. Search for information on the given topic (use web_search with links=true to discover source URLs)
  2. Fetch 2-4 relevant pages for detail using fetch_and_summarize (no more)
  3. Return a concise, structured summary of your findings
  ```
- **FR-3.3**: When `AICHAT_WSLINKS` is absent or false, `{{__researcher_search_instructions__}}` MUST resolve to direct grounded search instructions:
  ```text
  1. Search for information on the given topic using web_search (with links=false). The tool returns a grounded, comprehensive summary with source citations directly.
  2. Return a concise, structured summary of your findings based on the grounded search results. Do NOT fetch individual web pages.
  ```

### FR-4: Actuation Tool Guard in `tools/web_search_aichat.sh`
- **FR-4.1**: In `tools/web_search_aichat.sh`, the `--links` query override MUST be guarded by `AICHAT_WSLINKS`:
  `if [[ -n "$argc_links" ]] && [[ "${AICHAT_WSLINKS:-false}" == "true" ]]; then ... fi`
- **FR-4.2**: If `AICHAT_WSLINKS` is not `"true"`, any `--links` parameter passed to `web_search` MUST be ignored, ensuring that only the direct search query is executed with Google Search grounding.

### FR-5: Researcher Agent Definition & Harness Verification
- **FR-5.1**: `agents/researcher/index.yaml` MUST use `{{__researcher_search_instructions__}}` to dynamically adapt between direct grounded and link exploration modes.
- **FR-5.2**: `scripts/run-demos.nu` Demo 5 MUST explicitly pass `--wslinks` to verify the multi-step link discovery and `fetch_and_summarize` pipeline.
- **FR-5.3**: A companion Demo 5b MUST be added to `scripts/run-demos.nu` without `--wslinks` to verify that the direct grounded delegation completes in a single search turn per researcher without calling `fetch_and_summarize`.

## Non-Functional Requirements

- **Zero Overhead**: In direct search mode, turn count for research tasks drops from 5–8 turns to 2 turns, and token usage drops significantly.
- **Backward Compatibility**: Any script or harness invoking `aichat --wslinks` reproduces the exact link-scraping behavior without regression.
- **Process Isolation & Safety**: Does not alter capability masks, blast-radius authority gates, or mTLS escalation semantics.
