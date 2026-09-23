# Fork Enhancements Demo

Copy-paste examples demonstrating all new capabilities. Each command is self-contained — no config or tool file edits needed beyond the one-time setup below.

## Setup (one time)

```bash
# Point aichat/perry at the innators functions directory
export AICHAT_FUNCTIONS_DIR=~/projects/innators

# Enable web search (used by the researcher agent)
export WEB_SEARCH_MODEL="gemini:gemini-2.5-pro"

# Use the release binary (< /dev/null prevents stdin hang in non-interactive contexts)
alias aichat='~/projects/perry/target/release/perry'
alias perry='~/projects/perry/target/release/perry'
```

### Fix bin/ symlinks (if not already done)

The `bin/` directory must have symlinks to `scripts/run-tool.sh` (which converts JSON to CLI args), not directly to tool scripts:

```bash
cd ~/projects/innators
ls -la bin/fs_cat  # Should point to ../scripts/run-tool.sh

# If it points directly to tools/*.sh, fix all symlinks:
for tool in bin/*; do
    rm "$tool"
    ln -s ../scripts/run-tool.sh "$tool"
done
```

### Verify

```bash
aichat --list-agents
# Should show: coder, demo, json-viewer, orchestrator, researcher, sql, todo
```

All examples below assume the setup above is done.

---

## 1. Parallel Tool Execution

The model calls multiple tools in one turn — they execute concurrently instead of sequentially. A turn with 3 slow tasks takes ~2 seconds, not ~6.

```bash
aichat -r %functions% "You MUST call the slow_task tool exactly 3 times in parallel with labels 'alpha', 'beta', and 'gamma', each with delay 2. Report all results."
```

With trace enabled, you can see them start and finish together:

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true \
aichat -r %functions% \
  "You MUST call slow_task exactly 3 times in parallel: label='first' delay=2, label='second' delay=2, label='third' delay=2. Do NOT answer without calling the tools."
```

Expected trace output:
```
Agent %functions% (12345) loop trace:
  [12345 [turn 1/20] starting]
  [12345 calling: slow_task]
  [12345 calling: slow_task]
  [12345 calling: slow_task]
  [12345 slow_task completed (2.0s)]
  [12345 slow_task completed (2.0s)]
  [12345 slow_task completed (2.0s)]
  [12345 [turn 2/20] starting]
  [12345 done]
```

---

## 2. Turn Budget

The loop stops after `max_turns` and returns partial results instead of spinning forever.

```bash
# Budget of 1 turn: the model can call tools but never gets to respond
AICHAT_AGENT_LOOP_MAX_TURNS=1 \
aichat -r %functions% \
  "Read each of the files /etc/hostname, /etc/os-release, /etc/shells, /etc/fstab one by one and summarize each"
```

You'll see the stderr warning:
```
Warning: Agent loop reached the 1-turn limit without completing.
Increase with `agent_loop.max_turns` in config.yaml or AICHAT_AGENT_LOOP_MAX_TURNS=N.
```

---

## 3. Planning Tool (`_plan`)

The model uses `_plan` to reason before acting. Plan content appears in the trace but never in the output.

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true \
aichat -r %functions% \
  "This is a multi-step task. You MUST use the _plan tool first to plan your approach before taking any action. Then: read /etc/os-release, extract the distro name, and write a one-line summary to /tmp/os-summary.txt"
```

Look for the trace line:
```
  [12345 plan: "First I'll read /etc/os-release to see its contents, then..."]
```

The plan is invisible in the final response — only in the trace.

---

## 4. Sub-Agent Delegation

An orchestrator agent delegates tasks to specialist agents. Each runs as its own aichat process.

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true \
aichat --agent orchestrator \
  "You MUST delegate this to the researcher agent (do NOT answer yourself): Search the web for 'what is Model Context Protocol MCP by Anthropic' and return a summary with sources."
```

The trace shows delegation:
```
Agent orchestrator (12345) loop trace:
  [12345 [turn 1/20] starting]
  [12345 plan: "The user explicitly wants me to delegate the research tas..."]
  [12345 [turn 2/20] starting]
  [12345 calling: researcher]
Agent researcher (67890) loop trace:
  [67890 [turn 1/20] starting]
  [67890 calling: web_search]
  ...
  [67890 done]
  [12345 researcher completed (46.1s)]
  [12345 [turn 3/20] starting]
  [12345 done]
```

The researcher agent runs as a separate process with its own tools (`web_search`, `fetch_and_summarize`).

---

## 5. Recursive Orchestration (depth > 1)

Sub-agents can delegate to further sub-agents. Depth is bounded by `max_agent_depth` (default 3).

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true \
aichat --agent orchestrator \
  "You MUST delegate TWO separate research tasks (call the researcher agent twice in parallel): 1) 'Rust async runtimes 2025 comparison' 2) 'Python asyncio vs trio comparison'. Then synthesize both results."
```

The orchestrator spawns multiple researcher instances in parallel — each is an independent process with its own PID.

---

## 6. External Observability

### Terminal title (tmux)

In a tmux session, the pane title updates live:

```bash
# In tmux — watch the pane title change
aichat -r %functions% "Use fs_ls to list /usr/bin then report how many entries there are"
```

The title shows: `turn 1/20 | fs_ls | %functions%:12345 (2s)` → `done | %functions%:12345`

### Status file

While aichat is running, check its status from another terminal:

```bash
# In terminal 1:
aichat -r %functions% "Call slow_task with label 'long-running' and delay 10"

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
aichat -r %functions% "Use fs_cat to read the file /usr/share/dict/cracklib-small"
```

The file is ~492 KB. Since it exceeds 16 KB, the model receives:
```json
{"preview": "...(first 16KB)...", "full_output_path": "/tmp/aichat-tool-fs_cat-<pid>.out", "total_bytes": 492822, "hint": "52895 lines"}
```

The model can then use `fs_cat` with specific line ranges to access what it needs.

---

## 8. Tool Output Routing — Pipe Destination

The `fetch_and_summarize` tool is declared to pipe its output to `summarize_text`. The model never sees the raw HTML — only the summary.

```bash
aichat -r %functions% "You MUST call the fetch_and_summarize tool with url 'https://example.com'. Do not use any other tool."
```

Behind the scenes:
1. `fetch_and_summarize` fetches the URL → converts to clean Markdown via `html-to-markdown`
2. Output is piped to `summarize_text` → digest (5-8 bullet points)
3. Model sees only the digest, not the full Markdown

This saves tokens: a 50 KB page becomes a 200-byte summary in context.

---

## 9. Tool Output Routing — File Destination

Declare a tool to write its output to a file. The model gets a confirmation, not the content.

```bash
aichat -r %functions% "You MUST call generate_data with rows=20. Do NOT answer without calling the tool."
```

The `generate_data` tool is declared with `"output": {"destination": "file", "path": "/tmp/{{name}}-{{timestamp}}.csv"}` in functions.json. When called:

1. The tool generates 20 rows of CSV data
2. aichat routes the output to `/tmp/generate_data-<timestamp>.csv`
3. The model receives only a confirmation:

```json
{"written_to": "/tmp/generate_data-1724512321.csv", "size_bytes": 531, "hint": "21 lines"}
```

The full data goes to disk, never burning context tokens.

---

## 10. PDF Reading (structured Markdown)

Read PDFs as structured Markdown — headings, tables, lists preserved. Much better than flat text.

```bash
aichat -r %functions% "Use read_pdf to read the file ./manual.pdf and tell me what this document is about. List the main sections."
```

With page selection:
```bash
aichat -r %functions% "Use read_pdf to read pages 5-10 of ./manual.pdf in compact mode and summarize what those pages cover."
```

The `read_pdf` tool uses `pdf2md` — returns clean Markdown that the model can work with efficiently. Tables, headings, and formatting are preserved.

---

## 11. Combined: Full Agentic Workflow

Everything together — planning, parallel tools, sub-agents, observability:

```bash
AICHAT_AGENT_LOOP_SHOW_TRACE=true \
AICHAT_AGENT_LOOP_MAX_TURNS=15 \
aichat --agent orchestrator \
  "You MUST plan first using _plan, then delegate to the researcher agent: search the web for 'Model Context Protocol MCP Anthropic 2025' and return findings. Do NOT answer from memory — you MUST delegate."
```

What happens:
1. Orchestrator plans (via `_plan`)
2. Delegates research to `researcher` agent (subprocess with own PID)
3. Researcher tries `web_search`, circuit breaker trips after 3 failures
4. Researcher pivots to `fetch_and_summarize` (URL fetches with piped summarization)
5. Results return to orchestrator
6. Orchestrator synthesizes a final answer
7. Bell rings when done, title shows `done | orchestrator:12345`

---

## 12. Full Prompt & Model Dialog Observability (`--dialog` / `--no-truncate`)

Display exact prompts sent to every LLM and exact responses returned across all turns, subagents, background operations (session autonaming/compression), shell execution, and multi-agent loops.

```bash
# Live prompt & response dialog tracing
aichat --dialog "Analyze this repository"

# Live dialog tracing with full un-truncated history
aichat --dialog --no-truncate "Analyze this repository"

# Route dialog trace cleanly to stderr for script piping
AICHAT_DIALOG_OUTPUT=stderr aichat --dialog "Explain Rust traits" > output.md 2> dialog.log
```

Key features:
- **Model Attribution**: Displays configured model and wire model (`@ configured_model [wire: wire_model]`).
- **Semantic History Folding**: Folds prior multi-line history messages into compact summaries (`[history: tool_result fs_ls — 42 lines folded]`), preserving single-line indicators and errors.
- **Cross-Process Subagent Relay**: Subagents and generic shell tool child processes relay their dialog events over length-delimited stderr frames back to the parent sink without deadlocks.
- **Clean Redirection**: Supports `AICHAT_DIALOG_OUTPUT=stderr|tty` so standard output remains 100% clean for shell redirection.

---

## 13. Scoped Tool Selection (`-r %functions:tool1,tool2%`)

Instead of sending schemas for all 31 tools in every LLM turn (~6,000 tokens of boilerplate schema per request), scope the role down to only the tools needed for the task using `-r %functions:<tool1>,<tool2>%`.

```bash
# Run with only fs_cat and fs_write tools declared to the LLM:
aichat -r %functions:fs_cat,fs_write% "Inspect /etc/hostname and record the output to /tmp/hostname.txt"
```

Benefits:
- **Token Reduction**: Drops tool declaration overhead from ~6,000 tokens down to ~200-400 tokens per turn (~95% context reduction).
- **Latency & Cost**: Faster model time-to-first-token and lower operational cost.
- **Precision**: Limits the action space so the LLM does not hallucinate calls to unrelated tools.

---

## 14. Progressive Disclosure Runbooks & Skills (`read_skill`)

Rather than polluting the system prompt with entire playbooks and procedure guides, skills use **progressive disclosure**:
1. Eligible agents receive a compact catalog listing available skills (`### Available Skills`).
2. The agent discovers procedures dynamically and invokes `read_skill(name)` to fetch the runbook instructions only when needed.
3. **3-Tier Precedence**: Discovers skills with `workspace` (`.kiro/skills`, `.agents/skills`, `.skills`) > `global` (`~/.config/aichat/skills`) > `builtin` (`assets/builtin-skills`) precedence.
4. **Provenance Taint Tracking**: Runbooks loaded from the workspace are marked `WorkspaceTainted`. Loading an untrusted workspace runbook activates `untrusted_runbook: true` on the active plan step, feeding heightened scrutiny into `%assess-risk%` before mutating operations execute.

```bash
# Exercise builtin trusted triage skill (Demo 22):
nu scripts/run-demos.nu --demo 22

# Exercise workspace skill discovery and provenance taint tracking (Demo 23):
nu scripts/run-demos.nu --demo 23
```

---

## 15. Real-Time Model & Token Count Attribution

Every LLM request displays its estimated token footprint and invoked model badge directly in the live trace and dialog frames **before** network dispatch.

```bash
# Standard trace: displays tokens and yellow model tag at turn start
AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat -r %functions:fs_cat% "Read /etc/hostname"
```

Trace output:
```text
   +0.1s  [%functions:fs_cat% 169909 (PickyChaff) 103 tok @ gemini:gemini-2.5-flash [turn 1/5] starting]
```

In `--dialog` mode, token counts are rendered in `DarkGray` on both prompt submission frames (`📥`) and response frames (`📤`):
```text
┌── 📥 [169480 (LoopyUrchin) %functions:fs_cat,fs_write% 286 tok @ gemini:gemini-2.5-flash [turn 4/5] PROMPT SUBMITTED TO LLM]
...
┌── 📤 [169480 (LoopyUrchin) %functions:fs_cat,fs_write% 30 tok @ gemini:gemini-2.5-flash [turn 4/5] RESPONSE FROM LLM]
```

Benefits:
- Instant visibility into turn-by-turn context inflation.
- Visual attribution of exactly which model is handling root turns, subagents, and supervisory roles (`%assess-risk%`).

---

## 16. Evaluator Script & Command Formatting in `--dialog`

When `%assess-risk%` audits a state-mutating command or script, `--dialog` mode separates system auditor guidelines from the target action, unescaping underlying tool scripts and command strings into clean, readable Markdown syntax:

```text
Target File / Script: tools/fs_write.sh
Intent: execute tool 'fs_write'
Arguments:
  path: /tmp/patch.log

Script Source:
```bash
#!/usr/bin/env bash
...
```
```

The raw JSON sent to the evaluator LLM over the wire remains strictly byte-for-byte compliant, while the human operator sees human-readable, unescaped Bash rather than dense `\n`/`\"` JSON encoding.

---

## 17. Autonomy Ladder (`--autonomy <readonly|consult|reversible>`)

The Autonomy Ladder establishes operational postures across the 2D safety matrix (Capability Mask vs. Authority Ceiling):

### A. ReadOnly (`--autonomy readonly`)
Blocks all mutating tools at Gate 1 without invoking the LLM evaluator or prompting the human:
```bash
aichat --show-cost --autonomy readonly -r %functions:fs_write% \
  "Write 'TEST' to /tmp/blocked.txt using fs_write"
```
Output trace shows immediate Gate 1 denial:
```text
[BLOCK fs_write: read-only mask (mutating tool; unwound: true)]
```

### B. Reversible (`--autonomy reversible`)
Enables autonomous execution for actions that declare reversibility (`# @meta reversible-via backup`), recording an atomic rollback journal entry upfront:
```bash
aichat --show-cost --autonomy reversible -r %functions:fs_write% \
  "Write 'REVERSIBLE_TEST' to /tmp/remediated.txt using fs_write"
```
Output trace shows upfront Option B remediation and autonomous approval:
```text
[rollback journal: recorded fs_write (entry-...)]
[preflight remediation: fs_write (via backup -> stepped down to reversible)]
[ALLOW fs_write: risk reversible <= ceiling reversible]
```

### C. Consult (`--autonomy consult`)
Enforces the **Evaluator-First Unified Human Consultation Funnel**. Option B autonomous bypass is clamped; `%assess-risk%` audits the mutation upfront and presents a single, fully-informed prompt:
```bash
aichat --show-cost --autonomy consult -r %functions:fs_write% \
  "Write 'CONSULT_TEST' to /tmp/consult.txt using fs_write"
```

---

## Environment Variables Reference

| Variable | Effect |
|----------|--------|
| `AICHAT_AUTONOMY` | Autonomy posture preset: `readonly`, `consult`, or `reversible` |
| `AICHAT_FUNCTIONS_DIR` | Point at the llm-functions directory |
| `AICHAT_BUILTIN_SKILLS_DIR` | Override directory for builtin skills |
| `AICHAT_WORKSPACE_DIR` | Override workspace root for workspace skill discovery |
| `AICHAT_SAFETY_DEFAULT_CEILING` | Default authority ceiling (`safe`, `reversible`, `disruptive`, `destructive`) |
| `AICHAT_SAFETY_POLICY_FILE` | Path to Protected Policy File (`policy.yaml`) |
| `AICHAT_AGENT_LOOP_MAX_TURNS` | Override turn budget (default: 20) |
| `AICHAT_AGENT_LOOP_SHOW_TRACE` | Show live trace on terminal via `/dev/tty` (`true`/`false`) |
| `AICHAT_AGENT_LOOP_SHOW_DIALOG` | Show live prompt & response dialog trace (`true`/`false`) |
| `AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE` | Disable history folding in dialog trace (`true`/`false`) |
| `AICHAT_DIALOG_OUTPUT` | Override dialog destination: `stderr` (pipe-safe) or `tty` (default) |
| `AICHAT_DIALOG_RELAY` | Internal child-to-parent stderr relay trigger (`stderr`) |
| `AICHAT_AGENT_LOOP_MAX_COST` | Cost budget in USD (e.g. `1.0`). Stops loop if exceeded. |
| `AICHAT_AGENT_DEPTH` | (Set by aichat internally for sub-agents) |
| `WEB_SEARCH_MODEL` | Model for `web_search_perry` / `web_search` tool (e.g. `gemini:gemini-2.5-pro`) |
| `SUMMARIZE_MODEL` | Model for URL summarization (default: `gemini:gemini-3.6-flash`) |

## Config Reference (`agent_loop` section in config.yaml)

```yaml
agent_loop:
  max_turns: 20           # Turn budget
  max_concurrency: 8      # Parallel tool limit
  max_agent_depth: 3      # Sub-agent nesting depth
  show_trace: false       # Trace to /dev/tty (live, pipe-proof)
  show_dialog: false      # Prompt & response dialog trace (--dialog)
  dialog_no_truncate: false # Disable history folding (--no-truncate)
  planning_tool: true     # Inject _plan pseudo-tool
  osc_title: true         # Terminal title via /dev/tty (tmux pane title)
  status_file: true       # JSON status file ($XDG_RUNTIME_DIR/aichat-<pid>.json)
  notify: true            # BEL + OSC 777/9/99 notifications via /dev/tty
  tool_output_limit: 16384  # Auto-cap threshold (bytes, 0=disabled)
  max_cost: 0.0           # Cost budget in USD (0=unlimited). Stops loop if exceeded.
```

## tmux Configuration

Required for observability features:

```bash
# In ~/.config/tmux/tmux.conf
set -g allow-rename on        # Allow OSC title escape sequences
set -g monitor-bell on        # Highlight window on BEL notification
set -g status-right-length 80 # Room for title + hostname
# Add #T to show pane title in status bar:
set -g status-right "#[fg=magenta]#T #[fg=brightblack]#h"
```

## Troubleshooting

### Binary hangs on startup

The dev binary blocks on stdin when run non-interactively (e.g., from scripts). Redirect stdin:

```bash
# This hangs:
./target/release/aichat --list-agents

# This works:
./target/release/aichat --list-agents < /dev/null
```

The alias in the Setup section handles this automatically.

### Tools fail with "required arguments not provided"

The `bin/` symlinks must point to `scripts/run-tool.sh`, which converts JSON args to CLI flags. If they point directly to tool scripts, the scripts receive raw JSON and fail.

```bash
# Check:
ls -la ~/projects/innators/bin/slow_task
# Should show: bin/slow_task -> ../scripts/run-tool.sh

# Fix:
cd ~/projects/innators
for tool in bin/*; do rm "$tool"; ln -s ../scripts/run-tool.sh "$tool"; done
```

### Web search fails in researcher agent

Ensure `WEB_SEARCH_MODEL` is set (or configured in `config.yaml`). The `web_search_perry` tool uses it:

```bash
export WEB_SEARCH_MODEL="gemini:gemini-2.5-pro"
```
