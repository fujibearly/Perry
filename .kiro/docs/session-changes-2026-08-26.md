# Session Changes — 2026-08-26

Summary of all changes made during the demo testing and observability hardening session.

---

## aichat (Rust binary) — Source Changes

### 1. Observability: Write to `/dev/tty` (pipe-proof)

**Files**: `src/agent_loop.rs`

`update_terminal_title()`, `notify_terminal()`, and trace output all write directly to `/dev/tty`. This bypasses all stdout/stderr pipe/capture issues — works in tmux regardless of how the process is invoked (nushell `| complete`, backgrounded, piped stdin, etc.).

Falls back silently (no-op) when `/dev/tty` is unavailable (CI, containers, cron).

### 2. Trace output: Single channel, no duplication

**Files**: `src/agent_loop.rs`

Trace output (`AICHAT_AGENT_LOOP_SHOW_TRACE=true`) goes to exactly one place:
- If stdout is a terminal → `spinner.print_line()` (renders above spinner)
- If stdout is NOT a terminal → `/dev/tty` (live on terminal, not captured by pipes)
- If `/dev/tty` unavailable → `eprintln!` to stderr (fallback for CI/containers)

No more duplicate output. The trace you see scrolling live on your terminal IS the trace — there's no second copy in stderr.

### 3. Trace format: Agent label + PID per line

**Files**: `src/agent_loop.rs`

Trace header: `Agent orchestrator (12345) loop trace:`
Trace lines: `[12345 calling: researcher]`, `[12345 researcher completed (39.5s)]`

The label is:
- The agent name when using `--agent orchestrator` → `orchestrator`
- The role name when using `-r %functions%` → `%functions%`
- `aichat` as bare fallback (no role, no agent)

### 4. Notifications: Multi-protocol support

**Files**: `src/agent_loop.rs`

`notify_terminal()` emits four notification protocols via `/dev/tty`:

| Protocol | Terminals |
|----------|-----------|
| BEL (`\x07`) | Universal — tmux `monitor-bell`, terminal beep/flash |
| OSC 777 | Ghostty, iTerm2, rxvt-unicode, VS Code terminal |
| OSC 9 | Windows Terminal, ConEmu |
| OSC 99 | Kitty |

Terminals that don't understand a particular sequence silently ignore it.

### 5. Title format: Status first, identity last

**Files**: `src/agent_loop.rs`, `src/main.rs`, `src/repl/mod.rs`

Format: `turn 1/20 | researcher | orchestrator:12345 (8s)`

- Status (turn, active tools) comes first — most important info at a glance
- Agent/role label + PID + elapsed timer rightmost — changes less frequently
- Elapsed timer ticks every 2s via heartbeat (shows the process is alive)
- On completion: `done | orchestrator:12345`

Sub-agents (depth > 0) do NOT write to the pane title — only the root process owns it.

### 6. Title: Heartbeat updates during long-running tools

**Files**: `src/main.rs`

The `heartbeat.tick()` branch (every 2s) updates the terminal title. Previously, during a 30s researcher call, the title froze. Now the elapsed timer ticks continuously.

### 7. Stale status file cleanup at startup

**Files**: `src/agent_loop.rs`, `src/main.rs`

New `cleanup_stale_status_files()` called at startup. Scans `$XDG_RUNTIME_DIR` for `aichat-<pid>.json` files whose PIDs no longer exist (checked via `/proc/<pid>`). Safe for sub-agents: their PID exists while running, so their files are preserved.

### 8. Circuit breaker for tool failures

**Files**: `src/agent_loop.rs`

After 3 consecutive failures of the same tool, it's "tripped" — further calls return an immediate error:
```json
{"error": {"type": "circuit_breaker", "message": "Tool 'web_search_aichat' has been disabled after 3 consecutive failures. Use a different tool or approach."}}
```

The model receives this and can pivot to alternatives. A successful call resets the counter. Prevents runaway loops where a broken tool is retried for 20 turns.

### 9. File routing: Unwrap `{"output": "..."}` envelope

**Files**: `src/agent_loop.rs`

When a tool produces non-JSON text (e.g., CSV), aichat wraps it as `{"output": "..."}` for internal transport. For `file` destination routing, this wrapper is now stripped — the file contains clean raw content, not JSON.

### 10. Agent label from role name

**Files**: `src/main.rs`, `src/repl/mod.rs`

The agent label used in traces and tmux title comes from:
1. Agent name (if `--agent <name>`)
2. Role name (if `-r <role>`)
3. `"aichat"` (bare fallback)

### 11. Token cost accounting

**Files**: `src/agent_loop.rs`, `src/config/mod.rs`, `src/main.rs`

Full cost tracking across the agent loop:
- **Per-turn cost**: computed via `model.usage_cost()` after each LLM call, accumulated in `AgentLoopProgress`
- **Running cost in tmux title**: `turn 1/20 | researcher | orchestrator:12345 (38s $0.0420)`
- **Cost in status file**: `cost_usd` field in `aichat-<pid>.json`
- **Sub-agent cost aggregation**: parent passes `--show-cost` to sub-agents, parses cost from subprocess stderr, adds to its own total
- **Cost budget**: `agent_loop.max_cost` config field (or `AICHAT_AGENT_LOOP_MAX_COST` env var). When exceeded, loop stops with `CostExhausted` event and warning message.

The tmux title shows the **aggregate** cost — includes both the process's own LLM calls and all sub-agent spend.

---

## llm-functions — Tool/Agent Changes

### 1. `tools/generate_data.sh` (NEW)

Generates sample CSV data. Output routed to file by declaration.

### 2. `tools/fetch_url_via_curl.sh` (MODIFIED)

Uses `html-to-markdown` (v3.11.3) instead of `pandoc`. No external API dependency.
Output is piped through `summarize_text` via output routing declaration — the model receives a summary, not the full page.

### 3. `tools/summarize_text.sh` (REWRITTEN)

Previously a dumb word-counter. Now calls Gemini Flash (or configurable model) to produce a real LLM summary of web content (5-8 bullet points). Short content (<1KB) passes through unchanged.

Controlled by `SUMMARIZE_MODEL` env var (default: `gemini:gemini-3.6-flash`).

### 4. `agents/researcher/functions.json` (MODIFIED)

Tools: `web_search_aichat` + `fetch_url_via_curl` (replaced `web_search_tavily` + `fetch_url_via_jina`).
`fetch_url_via_curl` now has pipe routing to `summarize_text` — researcher receives summaries, not raw pages.

### 5. `agents/researcher/index.yaml` (MODIFIED)

Added explicit constraints to prevent excessive fetching:
- Max 5 URLs total per task
- Complete within 6-8 turns
- If a tool fails 3 times, stop retrying and switch approach
- Quality over quantity

### 6. `bin/` symlinks (FIXED)

All point to `../scripts/run-tool.sh` (JSON→CLI args conversion via `jq`).

### 7. `functions.json` — added `generate_data` + pipe routing for `fetch_url_via_curl`

- `generate_data`: output routing `{"destination": "file", "path": "/tmp/{{name}}-{{timestamp}}.csv"}`
- `fetch_url_via_curl`: output routing `{"destination": "pipe", "target": "summarize_text"}`

---

## Demo Script

**File**: `target/release/run-demos.nu`

Nushell script that runs all 11 demos. Features:
- `▶` command line shown for each demo
- Live trace output scrolls on terminal (via `/dev/tty`)
- `⚙ artifact` callouts for plans, capped files, CSV outputs
- `✓`/`✗` pass/fail verification checks
- `┄┄┄ output ┄┄┄` section with model response
- Demo 6 polls status file and tmux title mid-execution via background bash process

Uses `const SCRIPT_DIR = (path self | path dirname)` to resolve the release binary and project paths.

---

## Dependencies

### Rust Crate Dependencies (aichat binary)

No new crate dependencies. All changes use `std` only:
- `std::fs::OpenOptions` (for `/dev/tty`)
- `std::io::Write`
- `std::process::id()`
- `std::path::Path` (for `/proc/<pid>` check)
- `std::collections::{HashMap, HashSet}` (circuit breaker state)

### External Tool Dependencies (llm-functions)

| Tool | Version | Install | Used by |
|------|---------|---------|---------|
| `argc` | any | `cargo install argc` | All tool scripts (argument parser) |
| `jq` | any | System package | `scripts/run-tool.sh` (JSON→CLI arg conversion) |
| `curl` | any | System package | `fetch_url_via_curl`, `fetch_and_summarize` |
| `html-to-markdown` | 3.11.3 | `cargo install html-to-markdown-cli` | `fetch_url_via_curl` |
| `pdf2md` | any | `cargo install pdf2md` | `read_pdf` tool, document loader |
| `aichat` | this build | `cargo build --release` | `web_search_aichat` (calls itself with search model) |

### Runtime Environment

| Variable | Required by | Purpose |
|----------|-------------|---------|
| `AICHAT_FUNCTIONS_DIR` | All tool usage | Points to the llm-functions directory |
| `WEB_SEARCH_MODEL` | `web_search_aichat` | Model for grounded web search (e.g., `gemini:gemini-2.5-pro`) |
| `SUMMARIZE_MODEL` | `summarize_text` | Model for URL summarization (default: `gemini:gemini-3.6-flash`) |
| `AICHAT_AGENT_LOOP_SHOW_TRACE` | Trace output | `true` to enable live trace on terminal |
| `AICHAT_AGENT_LOOP_MAX_TURNS` | Turn budget | Override max turns (default: 20) |
| `AICHAT_AGENT_LOOP_MAX_COST` | Cost budget | Override max cost in USD (default: 0 = no limit) |
| `XDG_RUNTIME_DIR` | Status files | Where `aichat-<pid>.json` is written (falls back to `/tmp`) |

### tmux Configuration

Required in `~/.config/tmux/tmux.conf`:
```
set -g allow-rename on        # Allow OSC title escape sequences
set -g monitor-bell on        # Highlight window on BEL
```

Recommended for visibility:
```
set -g status-right "... #[fg=magenta]#T #[fg=brightblack]#h"
set -g status-right-length 80
```

---

## File Summary

### aichat repo (modified, uncommitted)
```
src/agent_loop.rs       — /dev/tty observability, circuit breaker, trace format, file routing fix, cost tracking
src/config/mod.rs       — max_cost config field, env var override, PartialEq fix
src/main.rs             — heartbeat title, stale cleanup, agent label from role, cost in title
src/repl/mod.rs         — agent label from role, idle title
enhancements-demo.md    — updated demo doc
target/release/run-demos.nu — nushell demo runner script
.kiro/architecture.md   — observability section updated
.kiro/docs/session-changes-2026-08-25.md — this file
```

### llm-functions repo (modified + new, uncommitted)
```
tools/generate_data.sh              — NEW: CSV generator for Demo 9
tools/fetch_url_via_curl.sh         — MODIFIED: uses html-to-markdown
tools/summarize_text.sh             — REWRITTEN: LLM summarization via Gemini Flash
agents/researcher/functions.json    — MODIFIED: web_search_aichat + fetch_url_via_curl
agents/researcher/index.yaml        — MODIFIED: constraints to prevent excessive fetching
functions.json                      — MODIFIED: generate_data + fetch_url_via_curl pipe routing
tools.txt                           — MODIFIED: added generate_data
bin/generate_data                   — NEW: symlink to ../scripts/run-tool.sh
```
