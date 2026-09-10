# Changelog

All notable changes to this fork are documented here. This project follows
[Semantic Versioning](https://semver.org/). Entries are grouped by fork release.

## 0.31.0-fork.9

### Added
- Live streaming progress for OpenAI Responses multi-agent turns.
- Terminal-safe spinner output: respects terminal width, supports `--no-spinner`,
  and can print progress lines without corrupting the spinner.
- Unified ALLOW / BLOCK governance nomenclature (`<VERB> <tool>: <lhs> <op> <rhs>`)
  with fixed operand ordering (`risk` on LHS, `ceiling` on RHS) across trace events.
- Single-source risk token formatting (`format_risk_token`) with parenthetical
  `(effective, <why>)` qualifiers for reversibility discounts, policy raises, and evaluator raises.
- Threaded authority ceiling into interactive human prompts with the non-colliding
  banner `[HUMAN APPROVAL REQUIRED] <tool>`.
- Debug rollback journal inspection with `--debug` / `AICHAT_AGENT_LOOP_DEBUG`
  displaying metadata inside guide rails while strictly omitting backup file contents.
- Deterministic human-readable agent petnames (`format_agent_pid(pid)`).
- Bounded helper script resolution in `%assess-risk%` evaluator context.
- Empty LLM response detection and automatic backoff retry in `call_llm_raw` (`MAX_EMPTY_RETRIES = 2`), preventing premature/silent loop completions.
- Extended Vertex AI / Gemini streaming parser to catch provider `blockReason` and `RECITATION` finish reasons.
- Added `--links` flag to `web_search` and updated `researcher` agent instructions to enable multi-step search-and-fetch workflows via `fetch_url_via_curl`.
- Fixed pre-step demo description and command rendering order in `scripts/run-demos.nu`.
- Unconditionally preserved tool results in `eval_tool_calls_parallel`, eliminating the `is_all_done` result drop that caused infinite turn-budget exhaustion loops in multi-turn agents.
- Enriched `tool_execution_error` messages with underlying error strings `{e}` for improved agent self-correction.
- Hardened `fetch_url_via_curl.sh` with `set -eo pipefail`, modern browser User-Agent, and 30-second connection timeout.
- Directed Google Search grounding in `web_search_aichat.sh` to return canonical direct URLs rather than ephemeral redirect tokens.

## 0.31.0-fork.8

### Changed
- OpenAI Responses multi-agent transport switched to server-sent events (SSE).
  Automatic reconnection is disabled to prevent replaying billable POSTs.
  Transient HTTP errors are retried only before the stream opens.

## 0.31.0-fork.7

### Added
- Hosted web search tool for Responses multi-agent. Configurable
  `search_context_size`, `external_web_access`, `return_token_budget`, and
  domain allow/block lists.
- Multi-agent structural trace (`--show-agent-trace` / `multi_agent.show_trace`).
- Exact per-turn Responses pricing with cached input, cache writes, service tier
  multipliers, and long-context thresholds.

## 0.31.0-fork.6

### Added
- OpenAI Responses multi-agent orchestration for GPT-5.6 models.
  New CLI flags: `--multi-agent`, `--max-concurrent-subagents`,
  `--max-output-tokens`, `--service-tier`.
- Multi-turn continuation loop (up to 64 turns) with function-call caching
  and infinite-loop detection.

## 0.31.0-fork.5

### Added
- Token usage cost summaries (`--show-cost` / `show_cost` config option).
  Displays provider-reported token counts and estimated USD cost on stderr.

## 0.31.0-fork.4

### Added
- Provider-layer refactor: typed `ChatEvent` streaming events (`Text`,
  `Reasoning`, `ToolCall`, `Usage`), `ProviderError` classification, and
  automatic retry on transient errors (rate limits, server failures).
- Declarative client registry replacing the opaque `register_client!` dispatch
  macro. Client structs and `init_client()` are now explicit match arms.
- Bounded Claude provider diagnostics: error details are sanitized before
  surfacing to the user.

### Changed
- Truncated or incomplete streams are now rejected as errors instead of being
  silently accepted.

## 0.31.0-fork.3

### Added
- Provider-neutral reasoning effort aliases. Append `:effort` to any model name
  (e.g. `openai:gpt-5.6:high`, `claude:claude-opus-4-7:medium`). Supported
  providers: OpenAI, Claude, VertexAI, Bedrock, Gemini. Unsupported efforts fail
  locally before hitting the API.

### Changed
- Cargo dependency lockfile refreshed.

## 0.31.0-fork.2

### Added
- Claude Fable 5 model catalog entries (reasoning efforts: low through max).
- Claude refusal response handling: refusal messages are extracted safely.
- Claude block lifecycle validation and bounded error diagnostics.
- Verified eight-record model catalog overlay.

## 0.31.0-fork.1

### Added
- Environment variable references in config (`$VAR`, `${VAR}`, literal `$$`).
- `ANTHROPIC_API_KEY` fallback for Claude client configuration.
- Verified upstream model catalog refresh (GPT-5.6 family, Claude Opus 4.7,
  Gemini updates, MiniMax M2.7).
- Explicit CLI file grammar (`--files` with `--` separator).
- Local agent setup documentation and deterministic installation guidance.

### Fixed
- Cohere v2 stream completion recognition.
- Truncated streaming completions are now rejected.
- Credential reference errors are propagated with clear messages.
- OpenAI tool call index tracking across streaming chunks.
- Claude streaming error events and overload error messages.
- Qwen3.6 close-only thinking tag stripping.
- Editor command resolution made deterministic (uses `which`).
- `cmd_prelude` applied before `--info` output.
- `$EDITOR` honored when config `editor` is unset.
- Role front-matter parsing made strict (replaces regex).
- Bedrock model ARN paths canonicalized for signing.
- Environment variable values trimmed of whitespace.
- RAG context refreshed during `.regenerate`.
- Tool call failures returned as structured results instead of panicking.
- Local media files read asynchronously.
- Stream flag always serialized in OpenAI-compatible requests.
- Shell exec output uses terminal yellow instead of hardcoded orange.

### Security
- Playground and Arena HTML sanitize rendered Markdown with DOMPurify (pinned,
  SRI integrity, fail-closed policy).
- Document loader placeholder expansion hardened against path injection.
- Ambiguous thinking markers preserved to prevent tag-stripping bypasses.

### Changed
- CI updated for Rust 1.97 compatibility.
- Provider streaming state machines completed for all backends.
