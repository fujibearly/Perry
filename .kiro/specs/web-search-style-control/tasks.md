# Web-Search Style & Branch-Wide Grounding Control (`--wslinks`) — Implementation Tasks

## Task 1: Add `--wslinks` CLI flag and environment variable handling
**Files:** `src/cli.rs`, `src/main.rs`
1. Add `pub wslinks: bool` to `Cli` struct in `src/cli.rs` with docstring explaining: "Enable link exploration mode for web searches (passes links: true and scrapes discovered URLs)".
2. In `src/main.rs` (early initialization before agent loop runs):
   - If `cli.wslinks` is `true`, execute `std::env::set_var("AICHAT_WSLINKS", "true")`.
3. Add unit test in `src/cli.rs` or `tests/` verifying that `--wslinks` parses correctly.

---

## Task 2: Propagate `--wslinks` to child subprocesses
**Files:** `src/agent_loop.rs`
1. In `eval_agent_tool_subprocess`:
   - Check if `std::env::var("AICHAT_WSLINKS").map(|v| v == "true" || v == "1").unwrap_or(false)`:
     - Add `cmd.arg("--wslinks")`.
     - Add `cmd.env("AICHAT_WSLINKS", "true")`.
2. Verify sub-agent inheritance across orchestrator -> researcher hierarchies.

---

## Task 3: Implement `{{__researcher_search_instructions__}}` template variable
**Files:** `src/utils/variables.rs`, `src/config/agent.rs`
1. In `src/utils/variables.rs` (or `interpolated_instructions()` in `src/config/agent.rs`):
   - When resolving `__researcher_search_instructions__`:
     - If `std::env::var("AICHAT_WSLINKS").map(|v| v == "true" || v == "1").unwrap_or(false)` is true, return:
       ```text
       1. Search for information on the given topic (use web_search with links=true to discover source URLs)
       2. Fetch 2-4 relevant pages for detail using fetch_and_summarize (no more)
       3. Return a concise, structured summary of your findings
       ```
     - Otherwise (default), return:
       ```text
       1. Search for information on the given topic using web_search (with links=false). The tool returns a grounded, comprehensive summary with source citations directly.
       2. Return a concise, structured summary of your findings based on the grounded search results. Do NOT fetch individual web pages.
       ```
2. Add unit tests verifying both branches of instruction resolution.

---

## Task 4: Add tool guard in `tools/web_search_aichat.sh`
**Files:** `/home/istari/projects/innators/tools/web_search_aichat.sh`
1. Guard the `$argc_links` check with `[[ "${AICHAT_WSLINKS:-false}" == "true" ]]`:
   ```bash
   if [[ -n "$argc_links" ]] && [[ "${AICHAT_WSLINKS:-false}" == "true" ]]; then
       query="Search the web for '$argc_query'. Do NOT write an essay or synthesized summary. Return ONLY a list of 3-5 source items formatted strictly as: [Page Title](Page URL) - 1-sentence summary. Use direct canonical target URLs (e.g. https://domain.com/path), never search engine redirect URLs."
   fi
   ```
2. If `AICHAT_WSLINKS` is absent or not `"true"`, ensure query remains `$argc_query` so `aichat` returns Google Search grounded summaries directly.

---

## Task 5: Update researcher agent instructions
**Files:** `/home/istari/projects/innators/agents/researcher/index.yaml`
1. Replace lines 6-8 in `index.yaml` with `{{__researcher_search_instructions__}}`.
2. Update constraints to note:
   - In direct search mode (default), synthesize directly from the grounded web_search result without fetching URLs.
   - In link-exploration mode, fetch at most 5 URLs total per task.
3. Run `argc build@agent researcher` in `innators`.

---

## Task 6: Update demo suite in `scripts/run-demos.nu`
**Files:** `scripts/run-demos.nu`
1. Update Demo 5 to explicitly pass `--wslinks` to test the multi-step delegation and link-exploration pipeline.
2. Add Demo 5b ("Parallel Delegation with Direct Grounded Search"):
   - Invokes orchestrator without `--wslinks`.
   - Verifies researchers call `web_search` with grounded answers and finish in fewer turns without calling `fetch_and_summarize`.

---

## Task 7: Verification and Quality Assurance
1. Run `cargo test --bin aichat -- --test-threads=1`.
2. Run `cargo clippy -- -D warnings`.
3. Run `argc test` in `llm-functions`.
4. Run `cargo build --release`.
5. Execute `nu scripts/run-demos.nu --demo 5 --no-truncate` and `nu scripts/run-demos.nu --demo 5b --no-truncate`.
6. Update documentation: `.kiro/docs/roadmap.md` and commit changes across both repositories.
