# Fork Enhancements Demo

Copy-paste examples demonstrating all new capabilities. Each command is self-contained — no config or tool file edits needed beyond the one-time setup below.

## Setup (one time)

```bash
# Point aichat at the dev functions directory
export AICHAT_FUNCTIONS_DIR=~/projects/llm-functions

# Verify tools are visible
aichat --list-agents
# Should show: coder, demo, json-viewer, orchestrator, researcher, sql, todo
```

All examples below assume `AICHAT_FUNCTIONS_DIR` is set.

---

## 1. Parallel Tool Execution

The model calls multiple tools in one turn — they execute concurrently instead of sequentially. A turn with 3 slow tasks takes ~2 seconds, not ~6.

```bash
aichat -r %functions% "Run slow_task three times in parallel with labels 'alpha', 'beta', and 'gamma'. Each should take 2 seconds. Report all results."
```

With trace enabled, you can see them start and finish together:

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat -r %functions% "Call slow_task three times with labels 'first', 'second', 'third' — all with 2 second delays"
```

Expected trace output:
```
Agent loop trace:
  [turn 1/20] starting
  [calling: slow_task]
  [calling: slow_task]
  [calling: slow_task]
  [slow_task completed (2.0s)]
  [slow_task completed (2.0s)]
  [slow_task completed (2.0s)]
  [done]
```

---

## 2. Turn Budget

The loop stops after `max_turns` and returns partial results instead of spinning forever.

```bash
# Set a very low budget to see it in action
AICHAT_AGENT_LOOP_MAX_TURNS=3 aichat -r %functions% "List files in the current directory, then read each .rs file one by one and summarize them"
```

You'll see the stderr warning:
```
Warning: Agent loop reached the 3-turn limit without completing.
Increase with `agent_loop.max_turns` in config.yaml or AICHAT_AGENT_LOOP_MAX_TURNS=N.
```

---

## 3. Planning Tool (`_plan`)

The model uses `_plan` to reason before acting. Plan content appears in the trace but never in the output.

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat -r %functions% "I have a file at /etc/os-release. Read it, then create a summary at /tmp/os-summary.md with the key facts formatted as a markdown table."
```

Look for the trace line:
```
  [plan: "First I'll read /etc/os-release to see its contents, then..."]
```

The plan is invisible in the final response — only in the trace.

---

## 4. Sub-Agent Delegation

An orchestrator agent delegates tasks to specialist agents. Each runs as its own aichat process.

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --agent orchestrator "What are the top 3 programming languages for embedded systems in 2026? Research this and give me a brief comparison."
```

The trace shows delegation:
```
Agent loop trace:
  [turn 1/20] starting
  [plan: "I'll delegate research to the researcher agent..."]
  [calling: researcher]
  [researcher completed (8.3s)]
  [done]
```

The researcher agent runs as a separate process with its own tools (`web_search_tavily`, `fetch_url_via_jina`).

---

## 5. Recursive Orchestration (depth > 1)

Sub-agents can delegate to further sub-agents. Depth is bounded by `max_agent_depth` (default 3).

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --agent orchestrator "Research Rust async patterns AND research Python async patterns. Compare the two approaches in a summary."
```

The orchestrator may spawn multiple researcher instances in parallel — each is an independent process with its own PID.

---

## 6. External Observability

### Terminal title (tmux)

In a tmux session, the pane title updates live:

```bash
# In tmux — watch the pane title change
aichat -r %functions% "List all files in /usr, then count how many there are"
```

The title shows: `aichat: turn 1/20 | fs_ls` → `aichat: done`

### Status file

While aichat is running, check its status from another terminal:

```bash
# In terminal 1:
aichat -r %functions% "Run slow_task with label 'long-running' and delay 10"

# In terminal 2 (while it's running):
cat /run/user/$(id -u)/aichat-$(pgrep -n aichat).json | jq .
```

Output:
```json
{
  "pid": 12345,
  "state": "working",
  "turn": 1,
  "max_turns": 20,
  "active_tools": ["slow_task"],
  "elapsed_s": 3.2,
  "updated_at": "1724512321"
}
```

### Desktop notification (bell)

With tmux `monitor-bell on`, switch to another pane and let aichat work:

```bash
# Enable bell monitoring in tmux
tmux set -g monitor-bell on

# Start a task, then switch to another pane (Ctrl-b n)
aichat -r %functions% "What time is it right now?"
```

When it completes, tmux highlights the aichat window in the status bar.

---

## 7. Tool Output Routing — Auto-Capping

Large tool results (>16 KB) are automatically capped: full content goes to a temp file, the model gets a preview + path.

```bash
aichat -r %functions% "Read the contents of /usr/share/dict/words"
```

If the file exceeds 16 KB, the model receives:
```json
{"preview": "...(first 16KB)...", "full_output_path": "/tmp/aichat-tool-fs_cat-12345.out", "total_bytes": 972563, "hint": "102401 lines"}
```

The model can then use `fs_cat` with specific line ranges to access what it needs.

---

## 8. Tool Output Routing — Pipe Destination

The `fetch_and_summarize` tool is declared to pipe its output to `summarize_text`. The model never sees the raw HTML — only the summary.

```bash
aichat -r %functions% "Use fetch_and_summarize to get the content of https://example.com"
```

Behind the scenes:
1. `fetch_and_summarize` fetches the URL → raw HTML
2. Output is piped to `summarize_text` → digest (word count, line count, preview)
3. Model sees only the digest, not the full HTML

This saves tokens: a 50 KB page becomes a 200-byte summary in context.

---

## 9. Tool Output Routing — File Destination

Declare a tool to write its output to a file. The model gets a confirmation, not the content.

To demo this, add this entry to your `functions.json`:

```json
{
  "name": "generate_data",
  "description": "Generate sample data",
  "parameters": {"type": "object", "properties": {"rows": {"type": "integer"}}},
  "output": {"destination": "file", "path": "/tmp/{{name}}-{{timestamp}}.csv"}
}
```

Then the model calling `generate_data` would get back:
```json
{"written_to": "/tmp/generate_data-1724512321.csv", "size_bytes": 4096, "hint": "50 lines"}
```

The full data goes to disk, never burning context tokens.

---

## 10. PDF Reading (structured Markdown)

Read PDFs as structured Markdown — headings, tables, lists preserved. Much better than flat text.

```bash
# Read a PDF (replace with any PDF path you have)
aichat -r %functions% "Read the PDF at ~/Downloads/some-document.pdf and summarize its key points"
```

With page selection:
```bash
aichat -r %functions% "Read pages 1-3 of ~/Downloads/some-document.pdf in compact mode and list the main topics"
```

The `read_pdf` tool uses `pdf2md` (firecrawl/pdf-inspector) — returns clean Markdown that the model can work with efficiently.

---

## 11. Combined: Full Agentic Workflow

Everything together — planning, parallel tools, sub-agents, observability:

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true \
AICHAT_AGENT_LOOP_MAX_TURNS=15 \
aichat --agent orchestrator \
  "Research what MCP (Model Context Protocol) is and find 3 popular MCP servers. Write a brief markdown guide explaining MCP to a beginner."
```

What happens:
1. Orchestrator plans (via `_plan`)
2. Delegates research to `researcher` agent (subprocess)
3. Researcher uses `web_search_tavily` + `fetch_url_via_jina` (parallel)
4. Results return to orchestrator
5. Orchestrator synthesizes a final guide
6. Bell rings when done, title shows "aichat: done"

---

## Environment Variables Reference

| Variable | Effect |
|----------|--------|
| `AICHAT_FUNCTIONS_DIR` | Point at the llm-functions directory |
| `AICHAT_AGENT_LOOP_MAX_TURNS` | Override turn budget (default: 20) |
| `AICHAT_AGENT_LOOP_SHOW_TRACE` | Show live trace on stderr (`true`/`false`) |
| `AICHAT_AGENT_DEPTH` | (Set by aichat internally for sub-agents) |

## Config Reference (`agent_loop` section in config.yaml)

```yaml
agent_loop:
  max_turns: 20           # Turn budget
  max_concurrency: 8      # Parallel tool limit
  max_agent_depth: 3      # Sub-agent nesting depth
  show_trace: false       # Trace events on stderr
  planning_tool: true     # Inject _plan pseudo-tool
  osc_title: true         # Terminal title updates
  status_file: true       # JSON status file
  notify: true            # Bell + desktop notification
  tool_output_limit: 16384  # Auto-cap threshold (bytes, 0=disabled)
```
