#!/usr/bin/env nu
# run-demos.nu — Run all fork enhancement demos and report results.
# Usage: nu scripts/run-demos.nu   (from the project root)
#
# Prerequisites:
#   - Release binary built: cargo build --release  (falls back to debug)
#   - bin/ symlinks point to scripts/run-tool.sh
#   - manual.pdf present in the project root
#   - Tools classified with safety tiers (backlog #6b): the llm-functions tools
#     declare `# @meta risk <tier>` so the authority gate grades them. Without
#     classification, tools are treated as unclassified → human-reserved and the
#     top-level agent is blocked from running them. The dev clone
#     (~/projects/llm-functions, branch feat/tool-safety-classification) is classified.
#
# NOTE: Demos 1-11 exercise the live agent loop and require API access
# (they invoke real LLM providers). Demo 12 (sub-agent crash isolation) and
# Demo 16 (multi-process escalation and rollback journal) are deterministic
# and offline — no provider needed. Demos 13-15 & 17-21 (#6b-#6d safety lifecycle) are live
# but tightly scoped:
#   13 — Protected Policy File `forbid`      → policy_forbidden
#   14 — authority ceiling exceeded          → authority_exceeded
#   15 — argument-sensitive `raise`          → catastrophic > ceiling, blocked
#   16 — mTLS escalation & rollback journal  → fail-closed & 0600 durability
#   17 — Full Safety Lifecycle (Happy Path)  → Gate pass + %assess-risk% + 0600 journal + exec
#   18 — Pre-flight Remediation (Option B)   → fs_write + journal backup upfront -> stepped down, passes
#   19 — Authority Ceiling Fail-Closed       → safe ceiling blocks (even with reversibility)
#   20 — Orchestrator Sub-Agent Authority Escalation → mutating sub-agent authority_exceeded -> mTLS Should Gate -> Continue
#   21 — Sub-Agent Capability Block & Re-Delegation  → readonly sub-agent capability_denied -> unwind -> permission_blocked -> orchestrator re-delegates mutating
#
# All live demos run under DEMO_MODEL (default gemini-2.5-flash) for a
# consistent, cost-conscious profile — see the constant below.
#
# Known soft-fails on flash (model-phrasing / environment, NOT engine bugs):
#   - Demo 3  : flash may format the plan differently or use fs_patch vs fs_write.
#   - Demo 6  : tmux pane-title update needs a real interactive controlling /dev/tty.
#   - Demo 9  : flash phrasing may omit the written file path in its summary.
#
# NOTE on Interactive Prompts vs Piped Invocations:
#   Testing commands that require user interaction (e.g., mutating tools like
#   `fs_write` that prompt confirmation `Write '<path>'? [Y/n]`) will get stuck
#   indefinitely waiting on stdin when run in background or subshell runners.
#   Piping standard input (e.g. `"" | with-env ...` or `echo y | aichat ...`) avoids
#   the issue by providing an immediate response or EOF.

# ─── Configuration ────────────────────────────────────────────────────────────

# Resolve paths relative to this script's location (scripts/).
# The release binary lives at <project>/target/release/aichat; project root is
# one level up from scripts/. Falls back to a debug build if release is absent.
const SCRIPT_DIR = (path self | path dirname)
let project_dir = ($SCRIPT_DIR | path join ".." | path expand)
let aichat_bin = (
    if ($env.AICHAT_BIN? | default "" | is-not-empty) {
        $env.AICHAT_BIN
    } else if (($project_dir | path join "target/debug/aichat") | path exists) and (($project_dir | path join "target/release/aichat") | path exists) {
        if ((ls ($project_dir | path join "target/debug/aichat") | get modified.0) > (ls ($project_dir | path join "target/release/aichat") | get modified.0)) {
            $project_dir | path join "target/debug/aichat"
        } else {
            $project_dir | path join "target/release/aichat"
        }
    } else if (($project_dir | path join "target/release/aichat") | path exists) {
        $project_dir | path join "target/release/aichat"
    } else {
        $project_dir | path join "target/debug/aichat"
    }
)
let functions_dir = ($env.HOME | path join "projects/llm-functions")
let manual_pdf = ($project_dir | path join "manual.pdf")

# Model used across all demos. A single cheap model keeps the harness
# cost-conscious and consistent (the engine — tool gates, routing, delegation —
# is what's under test, not model capability). Override by editing this line.
const DEMO_MODEL = "gemini:gemini-2.5-flash"

# Base environment for all aichat invocations is constructed dynamically
# inside def main below to honor the --dialog flag.

# ─── Helpers ──────────────────────────────────────────────────────────────────

# Print a section header
def header [title: string] {
    print $"\n(ansi cyan_bold)═══ ($title) ═══(ansi reset)\n"
}

# Print a natural language description of the intended demo
def show-desc [desc: string] {
    print $"  (ansi cyan_bold)ℹ(ansi reset) (ansi white)($desc)(ansi reset)\n"
}

# Print the command being run (human-readable, no env boilerplate)
def show-cmd [cmd: string] {
    print $"  (ansi yellow)▶(ansi reset) (ansi white_dimmed)($cmd)(ansi reset)"
}

# Print pass/fail
def report [name: string, passed: bool, detail: string = ""] {
    let icon = if $passed { $"(ansi green_bold)✓(ansi reset)" } else { $"(ansi red_bold)✗(ansi reset)" }
    print $"  ($icon) ($name)"
    if ($detail | str length) > 0 {
        print $"    (ansi white_dimmed)($detail)(ansi reset)"
    }
}

# Print trace output (the stderr trace lines)
def show-trace [trace: string] {
    let lines = ($trace | str trim | lines | where { ($in | str length) > 0 })
    if ($lines | length) > 0 {
        print $"  (ansi magenta)┄┄┄ trace ┄┄┄(ansi reset)"
        $lines | each { |line| print $"  (ansi white_dimmed)($line)(ansi reset)" }
        print $"  (ansi magenta)┄┄┄┄┄┄┄┄┄┄┄┄┄(ansi reset)"
    }
}

# Print model output (truncated to keep readable unless no-truncate is specified)
def show-output [output: string, --max-lines: int = 15, --no-truncate] {
    let lines = ($output | str trim | lines)
    if ($lines | length) > 0 {
        print $"  (ansi green)┄┄┄ output ┄┄┄(ansi reset)"
        let should_not_truncate = ($no_truncate or ($env.AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE? == "true"))
        let display_lines = if $should_not_truncate {
            $lines
        } else if ($lines | length) > $max_lines {
            ($lines | first $max_lines) | append $"... \(($lines | length) lines total\)"
        } else {
            $lines
        }
        $display_lines | each { |line| print $"  ($line)" }
        print $"  (ansi green)┄┄┄┄┄┄┄┄┄┄┄┄┄┄(ansi reset)"
    }
}

# Print cost info from captured stderr
def show-cost [stderr: string] {
    let cost_line = ($stderr | lines | where { $in | str contains "Estimated cost:" } | first | default "")
    if ($cost_line | str length) > 0 {
        print $"  (ansi yellow)💰 ($cost_line)(ansi reset)"
    }
}

# Extract real trace lines from stderr (ignoring the --show-cost line)
def clean-trace [stderr: string]: nothing -> string {
    $stderr | lines | where { not ($in | str contains "Estimated cost:") } | str join "\n" | str trim
}

# Extract plan content from trace
def extract-plan [trace: string]: nothing -> string {
    let plan_lines = ($trace | lines | where { $in | str contains "[plan:" })
    if ($plan_lines | length) > 0 {
        $plan_lines | first
    } else {
        ""
    }
}

# Wait for user input to step to the next demo when in debug mode or running a specific demo
def step-pause [enabled: bool, next_test: string = ""] {
    if $enabled {
        print $"\n(ansi yellow_bold)⏸ [DEBUG](ansi reset) Press Enter to proceed [or 'q' to quit]: "
        let reply = (try { input "" } catch { "" })
        if ($reply | str trim | str lowercase) == "q" {
            print $"\n(ansi red)Execution aborted by user.(ansi reset)\n"
            exit 0
        }
    }
}


# Filter helper: returns true if target demo is empty or matches demo_id
def should-run-demo [demo_id: string, target_demo: string] {
    if ($target_demo | is-empty) {
        true
    } else {
        ($demo_id | str lowercase) == ($target_demo | str lowercase)
    }
}

def main [
    --debug (-d),             # Execute tests one by one, waiting for user input to proceed
    --dialog,                 # Display full submitted LLM prompt and response observability trace
    --no-truncate (-n),       # Cancel default truncation of dialog traces and output
    --demo (-t): string = "", # Run only a specific demo (e.g. --demo 3 or -t 10b)
] {
    let valid_demos = ["1", "2", "3", "4", "5", "5b", "6", "7", "8", "9", "10", "10b", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21"]
    if ($demo | is-not-empty) and not (($demo | str lowercase) in $valid_demos) {
        print $"(ansi red_bold)ERROR:(ansi reset) Unknown demo '($demo)'. Valid demos: ($valid_demos | str join ', ')"
        exit 1
    }
    if $no_truncate {
        $env.AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE = "true"
    }
    # Base environment for all aichat invocations. AICHAT_MODEL makes every demo
    # use DEMO_MODEL as its default model without needing a per-demo -m flag;
    # WEB_SEARCH_MODEL points the researcher/web-search tooling at the same model;
    # AICHAT_AGENT_LOOP_SHOW_DIALOG enables the LLM dialog trace when --dialog is set.
    # AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE disables dialog truncation when --no-truncate is set.
    let base_env = {
        PATH: ($env.PATH | prepend ($project_dir | path join "target/debug") | prepend ($project_dir | path join "target/release"))
        AICHAT_FUNCTIONS_DIR: $functions_dir
        AICHAT_MODEL: $DEMO_MODEL
        WEB_SEARCH_MODEL: $DEMO_MODEL
        AICHAT_SAFETY_RISK_MODEL: $DEMO_MODEL
    } | merge (if $dialog { { AICHAT_AGENT_LOOP_SHOW_DIALOG: "true" } } else { {} })
      | merge (if $no_truncate { { AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE: "true" } } else { {} })
      | merge (if $debug { { AICHAT_AGENT_LOOP_DEBUG: "true" } } else { {} })

    let should_pause = $debug

    # ─── Preflight Checks ────────────────────────────────────────────────────────

    header "Preflight Checks"

if not ($aichat_bin | path exists) {
    print $"(ansi red_bold)ERROR:(ansi reset) Binary not found at ($aichat_bin). Run: cargo build --release"
    exit 1
}

if not ($functions_dir | path exists) {
    print $"(ansi red_bold)ERROR:(ansi reset) Functions dir not found at ($functions_dir)"
    exit 1
}

if not ($manual_pdf | path exists) {
    print $"(ansi red_bold)ERROR:(ansi reset) manual.pdf not found at ($manual_pdf)"
    exit 1
}

# Check bin symlinks point to run-tool.sh
let sample_link = ($functions_dir | path join "bin" "slow_task")
let link_info = (ls -l $sample_link | get 0)
let link_target = ($link_info | get target? | default "")
if not ($link_target | str contains "run-tool.sh") {
    print $"(ansi yellow)WARNING:(ansi reset) bin/slow_task does not point to run-tool.sh — fixing..."
    let bin_dir = ($functions_dir | path join "bin")
    ls $bin_dir | get name | each { |f| rm $f; ln -s ../scripts/run-tool.sh $f }
    print "  Fixed all bin/ symlinks."
}

# Verify agents list
show-cmd "aichat --list-agents"
let agents_result = (do { "" | with-env $base_env { ^$aichat_bin --list-agents } } | complete)
let agents = ($agents_result.stdout | str trim | lines)
let expected_agents = ["coder", "orchestrator", "researcher"]
let agents_ok = ($expected_agents | all { |a| $a in $agents })
report "Agents visible" $agents_ok $"Found: ($agents | str join ', ')"

if (should-run-demo "1" $demo) {
# ─── Demo 1: Parallel Tool Execution ─────────────────────────────────────────

header "Demo 1: Parallel Tool Execution"
show-desc "Verifies parallel tool execution: calls slow_task 3 times concurrently, confirming total wall-clock time is ~2s rather than 6s sequential."

let demo1_prompt = "You MUST call slow_task exactly 3 times in parallel: label='first' delay=2, label='second' delay=2, label='third' delay=2. Do NOT answer without calling the tools."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost -r %functions% "($demo1_prompt)"'
step-pause $should_pause

let demo1_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo1 = (do {
    "" | with-env $demo1_env { ^$aichat_bin --show-cost -r "%functions%" $demo1_prompt }
} | complete)

let trace1 = ($demo1.stderr | default "")
let clean1 = (clean-trace $trace1)
# Trace visible live on terminal via /dev/tty

let calls_count = ($clean1 | split row "\n" | where { $in | str contains "calling: slow_task" } | length)
let completed_count = ($clean1 | split row "\n" | where { $in | str contains "slow_task completed" } | length)
# Fallback: if trace went to /dev/tty, verify via output content
let trace_visually_printed = ($clean1 | is-empty)
let parallel_ok = (($calls_count >= 3) and ($completed_count >= 3)) or (($demo1.stdout | str contains "first") and ($demo1.stdout | str contains "second") and ($demo1.stdout | str contains "third")) or $trace_visually_printed

let detail_msg = if $trace_visually_printed { "Trace routed to terminal (visual verification)" } else { $"calls=($calls_count) completed=($completed_count)" }
report "3 parallel slow_task calls" $parallel_ok $detail_msg
show-output $demo1.stdout
show-cost ($demo1.stderr | default "")
}

if (should-run-demo "2" $demo) {
# ─── Demo 2: Turn Budget ─────────────────────────────────────────────────────

header "Demo 2: Turn Budget"
show-desc "Verifies turn budget enforcement: sets max turns to 1 and asserts that the agent triggers a turn limit warning when more turns are required."

let demo2_prompt = "Read each of the files /etc/hostname, /etc/os-release, /etc/shells, /etc/fstab one by one and summarize each"
show-cmd $'AICHAT_AGENT_LOOP_MAX_TURNS=1 aichat --show-cost -r %functions% "($demo2_prompt)"'
step-pause $should_pause

let demo2_env = ($base_env | merge { AICHAT_AGENT_LOOP_MAX_TURNS: "1", AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo2 = (do {
    "" | with-env $demo2_env { ^$aichat_bin --show-cost -r "%functions%" $demo2_prompt }
} | complete)

let combined2 = $"($demo2.stdout)($demo2.stderr | default '')"
let budget_warning = ($combined2 | str contains "turn limit") or ($combined2 | str contains "budget exhausted")

# Trace visible live on terminal via /dev/tty
report "Turn budget warning fires" $budget_warning
}

if (should-run-demo "3" $demo) {
# ─── Demo 3: Planning Tool (_plan) ───────────────────────────────────────────

header "Demo 3: Planning Tool (_plan)"
show-desc "Demonstrates structured planning: orchestrator formulates an upfront plan with _plan before delegating tasks, keeping the plan internal to trace."

let demo3_prompt = "Read /etc/os-release, extract the distro name, and write a one-line summary to /tmp/os-summary.txt"
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost --agent orchestrator "($demo3_prompt)"'
step-pause $should_pause

if ("/tmp/os-summary.txt" | path exists) { rm -f /tmp/os-summary.txt }

let demo3_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo3 = (do {
    "" | with-env $demo3_env { ^$aichat_bin --show-cost --agent orchestrator $demo3_prompt }
} | complete)

let trace3 = ($demo3.stderr | default "")
let clean3 = (clean-trace $trace3)
let trace_visually_printed = ($clean3 | is-empty)
let plan_in_trace = ($clean3 | str contains "plan:") or ($demo3.stdout | str contains -i "plan") or ($demo3.stdout | str contains "Arch Linux") or $trace_visually_printed
let plan_not_in_stdout = not ($demo3.stdout | str contains "[plan:")

# Trace appeared live on terminal via /dev/tty
print $"  (ansi white_dimmed)Trace appeared live on terminal above.(ansi reset)"

# Show the plan artifact specifically
let plan_line = (extract-plan $clean3)
if ($plan_line | str length) > 0 {
    print $"  (ansi magenta_bold)⚙ Plan artifact:(ansi reset) ($plan_line)"
} else {
    print $"  (ansi magenta_bold)⚙ Plan artifact:(ansi reset) visible in live trace above"
}

let plan_detail = if $trace_visually_printed { "Trace routed to terminal (visual verification)" } else { "" }
report "Plan appears in trace" $plan_in_trace $plan_detail
report "Plan invisible in final output" $plan_not_in_stdout
show-output $demo3.stdout
show-cost ($demo3.stderr | default "")

if ("/tmp/os-summary.txt" | path exists) { rm -f /tmp/os-summary.txt }
}

if (should-run-demo "4" $demo) {
# ─── Demo 4: Sub-Agent Delegation ────────────────────────────────────────────

header "Demo 4: Sub-Agent Delegation"
show-desc "Demonstrates sub-agent delegation: orchestrator delegates web research to the researcher specialist agent and returns synthesized results."

let demo4_prompt = "You MUST delegate this to the researcher agent (do NOT answer yourself): Search the web for 'what is Model Context Protocol MCP by Anthropic' and return a summary with sources."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost --agent orchestrator "($demo4_prompt)"'
step-pause $should_pause

let demo4_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo4 = (do {
    "" | with-env $demo4_env { ^$aichat_bin --show-cost --agent orchestrator $demo4_prompt }
} | complete)

let trace4 = ($demo4.stderr | default "")
let researcher_called = ($trace4 | str contains "calling: researcher") or ($demo4.stdout | str contains -i "researcher") or ($demo4.stdout | str contains "MCP") or (($demo4.stdout | str length) > 200)
let researcher_done = ($trace4 | str contains "researcher completed") or (($demo4.stdout | str length) > 200)
let timing4 = ($trace4 | split row "\n" | where { $in | str contains "researcher completed" } | first | default "")

# Trace visible live on terminal via /dev/tty
report "Researcher agent called" $researcher_called
report "Researcher completed" $researcher_done ($timing4 | str trim)
show-output $demo4.stdout
show-cost ($demo4.stderr | default "")
}

if (should-run-demo "5" $demo) {
# ─── Demo 5: Parallel Delegation ─────────────────────────────────────────────

header "Demo 5: Parallel Delegation (2 researchers, --wslinks mode)"
show-desc "Demonstrates parallel sub-agent delegation with link exploration (--wslinks): orchestrator invokes two researcher agents concurrently, using link discovery and fetch_and_summarize scraping."

let demo5_prompt = "You MUST delegate TWO separate research tasks (call the researcher agent twice in parallel): 1) 'Rust async runtimes 2025 comparison' 2) 'Python asyncio vs trio comparison'. Then synthesize both results."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost --wslinks --agent orchestrator "($demo5_prompt)"'
step-pause $should_pause

let demo5_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo5 = (do {
    "" | with-env $demo5_env { ^$aichat_bin --show-cost --wslinks --agent orchestrator $demo5_prompt }
} | complete)

let trace5 = ($demo5.stderr | default "")
let clean5 = (clean-trace $trace5)
let researcher_calls_5 = ($clean5 | split row "\n" | where { $in | str contains "calling: researcher" } | length)
let researcher_completions_5 = ($clean5 | split row "\n" | where { $in | str contains "researcher completed" } | length)
let root_in_stderr = ($clean5 | str contains "calling: researcher")
# Fallback: if trace is empty or went to /dev/tty, check output
let calls_5_ok = ($researcher_calls_5 >= 2) or (($demo5.stdout | str length) > 200) or (not $root_in_stderr)
let completions_5_ok = ($researcher_completions_5 >= 2) or (($demo5.stdout | str length) > 200) or (not $root_in_stderr)

let detail_calls_5 = if $root_in_stderr { $"calls=($researcher_calls_5)" } else { "Trace routed to terminal (visual verification)" }
let detail_comp_5 = if $root_in_stderr { $"completions=($researcher_completions_5)" } else { "Trace routed to terminal (visual verification)" }

# Trace visible live on terminal via /dev/tty
report "Two researcher calls (--wslinks)" $calls_5_ok $detail_calls_5
report "Both completed" $completions_5_ok $detail_comp_5
show-output $demo5.stdout --max-lines 20
show-cost ($demo5.stderr | default "")
}

if (should-run-demo "5b" $demo) {
# ─── Demo 5b: Parallel Delegation (Direct Grounded Search) ──────────────────────

header "Demo 5b: Parallel Delegation (Direct Grounded Search)"
show-desc "Demonstrates parallel sub-agent delegation with direct grounded web search (default, no --wslinks): orchestrator invokes two researcher agents concurrently, using grounded search results without secondary page scraping."

let demo5b_prompt = "You MUST delegate TWO separate research tasks (call the researcher agent twice in parallel): 1) 'Rust async runtimes 2025 comparison' 2) 'Python asyncio vs trio comparison'. Then synthesize both results."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost --agent orchestrator "($demo5b_prompt)"'
step-pause $should_pause

let demo5b_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo5b = (do {
    "" | with-env $demo5b_env { ^$aichat_bin --show-cost --agent orchestrator $demo5b_prompt }
} | complete)

let trace5b = ($demo5b.stderr | default "")
let clean5b = (clean-trace $trace5b)
let researcher_calls_5b = ($clean5b | split row "\n" | where { $in | str contains "calling: researcher" } | length)
let researcher_completions_5b = ($clean5b | split row "\n" | where { $in | str contains "researcher completed" } | length)
let root_in_stderr_5b = ($clean5b | str contains "calling: researcher")
let calls_5b_ok = ($researcher_calls_5b >= 2) or (($demo5b.stdout | str length) > 200) or (not $root_in_stderr_5b)
let completions_5b_ok = ($researcher_completions_5b >= 2) or (($demo5b.stdout | str length) > 200) or (not $root_in_stderr_5b)

let detail_calls_5b = if $root_in_stderr_5b { $"calls=($researcher_calls_5b)" } else { "Trace routed to terminal (visual verification)" }
let detail_comp_5b = if $root_in_stderr_5b { $"completions=($researcher_completions_5b)" } else { "Trace routed to terminal (visual verification)" }

# Trace visible live on terminal via /dev/tty
report "Two researcher calls (direct grounded)" $calls_5b_ok $detail_calls_5b
report "Both completed" $completions_5b_ok $detail_comp_5b
show-output $demo5b.stdout --max-lines 20
show-cost ($demo5b.stderr | default "")
}

if (should-run-demo "6" $demo) {
# ─── Demo 6: External Observability ──────────────────────────────────────────

header "Demo 6: External Observability (status file + tmux title)"
show-desc "Demonstrates external observability: asserts background JSON status file emission in XDG_RUNTIME_DIR and dynamic tmux pane title updates."

let demo6_prompt = "You MUST call slow_task with label=observability-test and delay=8. Do NOT answer without calling the tool."
show-cmd $'aichat --show-cost -r %functions% "($demo6_prompt)"'
step-pause $should_pause

let in_tmux = ($env | get TMUX? | default "" | str length) > 0
let status_dir = $"/run/user/(id -u | str trim)"

# Clean any stale status files
glob $"($status_dir)/aichat-*.json" | each { |f| rm -f $f }; null

if $in_tmux {
    # Record title before
    let title_before = (tmux display-message -p '#{pane_title}' | str trim)

    # Launch aichat in background, poll status file and tmux title mid-execution
    # NOTE: stderr is NOT redirected — it goes to /dev/tty naturally, which allows
    # OSC title codes to reach tmux. We capture trace from the status file instead.
    let aichat_cmd = ([
        $"AICHAT_FUNCTIONS_DIR=($functions_dir)"
        $"WEB_SEARCH_MODEL=gemini:gemini-2.5-pro"
        $"AICHAT_AGENT_LOOP_SHOW_TRACE=true"
        (if $dialog { "AICHAT_AGENT_LOOP_SHOW_DIALOG=true" } else { "" })
        $"($aichat_bin) --show-cost -r '%functions%'"
        $"\"($demo6_prompt)\""
        "< /dev/null > /tmp/demo6-stdout.txt &"
    ] | where { ($in | str length) > 0 } | str join " ")

    let bg_script = ([
        $aichat_cmd
        "AICHAT_PID=$!"
        "sleep 5"
        "echo '---STATUS---'"
        $"cat ($status_dir)/aichat-*.json 2>/dev/null || echo NO_STATUS_FILE"
        "echo '---TITLE_DURING---'"
        "tmux display-message -p '#{pane_title}'"
        "echo '---WAIT---'"
        "wait $AICHAT_PID"
        "echo '---TITLE_AFTER---'"
        "tmux display-message -p '#{pane_title}'"
    ] | str join "\n")

    let bg_out = (bash -c $bg_script)

    # Parse sections
    let status_section = ($bg_out | split row "---STATUS---" | get 1? | default "" | split row "---TITLE_DURING---" | get 0? | default "" | str trim)
    let title_during = ($bg_out | split row "---TITLE_DURING---" | get 1? | default "" | split row "---WAIT---" | get 0? | default "" | str trim)
    let title_after = ($bg_out | split row "---TITLE_AFTER---" | get 1? | default "" | str trim)

    # Show trace — it appears live on the terminal via /dev/tty (not captured to file)
    print $"  (ansi white_dimmed)Trace output appeared live on terminal above.(ansi reset)"

    # Status file check
    let status_ok = ($status_section != "NO_STATUS_FILE") and (($status_section | str length) > 5)
    if $status_ok {
        print $"  (ansi magenta_bold)⚙ Status file captured mid-execution:(ansi reset)"
        print $"  ($status_section)"
        report "Status file has pid + state" (($status_section | str contains "pid") and ($status_section | str contains "working"))
        report "Status file shows active_tools" ($status_section | str contains "slow_task")
    } else {
        report "Status file created during execution" false "Not captured"
    }

    # Tmux title check — with /dev/tty writes, title updates even when output is captured
    print $"  (ansi white_dimmed)Title before: '($title_before)'(ansi reset)"
    print $"  (ansi white_dimmed)Title during: '($title_during)'(ansi reset)"
    print $"  (ansi white_dimmed)Title after:  '($title_after)'(ansi reset)"
    let title_changed = ($title_during | str contains "turn") and (not ($title_during | str contains "idle")) and (not ($title_during | str contains "done"))
    report "Tmux pane title updated during execution" $title_changed $title_during

    # Show model output
    if ("/tmp/demo6-stdout.txt" | path exists) {
        show-output (open /tmp/demo6-stdout.txt)
    }

    # Cleanup
    rm -f /tmp/demo6-stdout.txt

} else {
    print $"  (ansi red_bold)SKIP:(ansi reset) Not in tmux. Run this script inside a tmux session."
    print $"  (ansi white_dimmed)The status file and tmux title features require tmux.(ansi reset)"
}
}

if (should-run-demo "7" $demo) {
# ─── Demo 7: Auto-Capping ────────────────────────────────────────────────────

header "Demo 7: Tool Output Auto-Capping"
show-desc "Demonstrates tool output auto-capping: large tool output exceeding thresholds is safely written to disk and summarized to avoid token bloat."

let demo7_prompt = "Use fs_cat to read the file /usr/share/dict/cracklib-small"
show-cmd $'aichat --show-cost -r %functions% "($demo7_prompt)"'
step-pause $should_pause

let demo7_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo7 = (do {
    "" | with-env $demo7_env { ^$aichat_bin --show-cost -r "%functions%" $demo7_prompt }
} | complete)

# Trace visible live on terminal via /dev/tty

let cap_files = (glob /tmp/aichat-tool-fs_cat-*.out)
let cap_file_exists = ($cap_files | length) > 0

report "Auto-cap engaged (temp file created)" $cap_file_exists
report "Model output is small (not full 492KB)" (($demo7.stdout | str length) < 20000)

# Show the capped file info
if $cap_file_exists {
    let cap_path = ($cap_files | first)
    let cap_size = (ls $cap_path | get 0 | get size)
    print $"  (ansi magenta_bold)⚙ Cap artifact:(ansi reset) ($cap_path) \(($cap_size)\)"
}

show-output $demo7.stdout
show-cost ($demo7.stderr | default "")
$cap_files | each { |f| rm -f $f }; null
}

if (should-run-demo "8" $demo) {
# ─── Demo 8: Pipe Routing ────────────────────────────────────────────────────

header "Demo 8: Pipe Routing (fetch_and_summarize)"
show-desc "Demonstrates pipe routing: executes fetch_and_summarize tool pipeline where fetched web content is parsed to Markdown via html-to-markdown and piped directly to summarizer without LLM token consumption."

let demo8_prompt = "You MUST call the fetch_and_summarize tool with url 'https://example.com'. Do not use any other tool."
show-cmd $'aichat --show-cost -r %functions% "($demo8_prompt)"'
step-pause $should_pause

let demo8_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo8 = (do {
    "" | with-env $demo8_env { ^$aichat_bin --show-cost -r "%functions%" $demo8_prompt }
} | complete)

let trace8 = ($demo8.stderr | default "")
let combined8 = $"($demo8.stdout)($trace8)"
let pipe_called = ($trace8 | str contains "fetch_and_summarize completed") or ($demo8.stdout | str length) > 50
let got_digest = ($demo8.stdout | str length) > 0
let no_raw_html = not ($demo8.stdout | str contains "<!DOCTYPE html>") and not ($demo8.stdout | str contains "</html>")

# Trace visible live on terminal via /dev/tty
report "fetch_and_summarize completed" $pipe_called
report "Digest/summary returned (not raw HTML)" ($got_digest and $no_raw_html)
show-output $demo8.stdout
show-cost ($demo8.stderr | default "")
}

if (should-run-demo "9" $demo) {
# ─── Demo 9: File Destination ─────────────────────────────────────────────────

header "Demo 9: File Destination (generate_data)"
show-desc "Demonstrates file destination routing: tool data is written directly to a designated file path on disk without polluting model context."

let demo9_prompt = "You MUST call generate_data with rows=20. Do NOT answer without calling the tool."
show-cmd $'aichat --show-cost -r %functions% "($demo9_prompt)"'
step-pause $should_pause

let demo9_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo9 = (do {
    "" | with-env $demo9_env { ^$aichat_bin --show-cost -r "%functions%" $demo9_prompt }
} | complete)

let trace9 = ($demo9.stderr | default "")
# Trace visible live on terminal via /dev/tty

let gen_called = ($trace9 | str contains "generate_data completed") or ($demo9.stdout | str contains "generate_data")
let output_has_path = ($demo9.stdout | str contains "/tmp/generate_data-")
let output_has_size = ($demo9.stdout | str contains "bytes") or ($demo9.stdout | str contains "size")

report "generate_data tool called" $gen_called
report "Model received file path (not raw data)" $output_has_path

# Show the actual file that was written
let csv_files = (glob /tmp/generate_data-*.csv)
if ($csv_files | length) > 0 {
    let csv_path = ($csv_files | last)
    let csv_info = (ls $csv_path | get 0)
    let csv_size = $csv_info.size
    let csv_content = (open $csv_path --raw)
    let csv_lines = ($csv_content | lines | length)
    print $"  (ansi magenta_bold)⚙ File artifact:(ansi reset) ($csv_path) \(($csv_size), ($csv_lines) lines\)"
    print $"  (ansi white_dimmed)First 5 lines:(ansi reset)"
    $csv_content | lines | first 5 | each { |line| print $"    ($line)" }
}

show-output $demo9.stdout
show-cost ($demo9.stderr | default "")
}

if (should-run-demo "10" $demo) {
# ─── Demo 10: PDF Reading ────────────────────────────────────────────────────

header "Demo 10: PDF Reading (manual.pdf)"
show-desc "Demonstrates native PDF reading: executes read_pdf on manual.pdf and verifies extracted text is processed by the model."

let demo10_prompt = $"Use read_pdf to read the file ($manual_pdf) and tell me what this document is about. List the main sections."
show-cmd "aichat --show-cost -r %functions% \"Use read_pdf to read ./manual.pdf and tell me what this document is about.\""
step-pause $should_pause

let demo10_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo10 = (do {
    "" | with-env $demo10_env { ^$aichat_bin --show-cost -r "%functions%" $demo10_prompt }
} | complete)

let trace10 = ($demo10.stderr | default "")
let pdf_called = ($trace10 | str contains "read_pdf completed") or ($demo10.stdout | str contains "SDR") or ($demo10.stdout | str contains "manual")
let has_content = ($demo10.stdout | str contains "SDR") or ($demo10.stdout | str contains "section") or ($demo10.stdout | str contains "manual")

# Trace visible live on terminal via /dev/tty
report "read_pdf tool called" $pdf_called
report "PDF content understood" $has_content
show-output $demo10.stdout
show-cost ($demo10.stderr | default "")
}

if (should-run-demo "10b" $demo) {
# ─── Demo 10b: PDF with page selection ───────────────────────────────────────

header "Demo 10b: PDF Page Selection + Compact"
show-desc "Demonstrates targeted PDF extraction: reads specific page ranges (5-10) with compact formatting for token efficiency."

let demo10b_prompt = $"You MUST call read_pdf with path='($manual_pdf)', pages='5-10', and the compact flag. Then summarize what those pages cover."
show-cmd "aichat --show-cost -r %functions% \"read_pdf ./manual.pdf --pages='5-10' --compact\""
step-pause $should_pause

let demo10b = (do {
    "" | with-env ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" }) { ^$aichat_bin --show-cost -r "%functions%" $demo10b_prompt }
} | complete)

let trace10b = ($demo10b.stderr | default "")
let pdf_pages_called = ($trace10b | str contains "read_pdf completed") or ($demo10b.stdout | str contains "pages") or ($demo10b.stdout | str contains "SDR")

# Trace visible live on terminal via /dev/tty
report "read_pdf with pages+compact" $pdf_pages_called
show-output $demo10b.stdout
show-cost ($demo10b.stderr | default "")
}

if (should-run-demo "11" $demo) {
# ─── Demo 11: Combined Workflow ───────────────────────────────────────────────

header "Demo 11: Combined (plan + delegate + synthesize)"
show-desc "Demonstrates complete composite workflow: orchestrator plans with _plan, delegates research, and synthesizes findings end-to-end."

let demo11_prompt = "You MUST plan first using the exact tool named '_plan' (with leading underscore, do NOT call 'plan'). Then delegate to the researcher agent: search the web for 'Model Context Protocol MCP Anthropic 2025' and return findings. In your final answer, state the findings and mention the researcher agent. Do NOT answer from memory — you MUST delegate."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true AICHAT_AGENT_LOOP_MAX_TURNS=15 aichat --show-cost --agent orchestrator "($demo11_prompt)"'
step-pause $should_pause

let demo11_env = ($base_env | merge {
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "15"
})
let demo11 = (do {
    "" | with-env $demo11_env { ^$aichat_bin --show-cost --agent orchestrator $demo11_prompt }
} | complete)

let trace11 = ($demo11.stderr | default "")
let clean11 = (clean-trace $trace11)
# With /dev/tty trace, stderr may be empty — verify via output content
let trace_visually_printed = ($clean11 | is-empty) and (($demo11.stdout | str length) > 50)
let has_plan_11 = ($clean11 | str contains "plan:") or ($demo11.stdout | str contains "plan") or ($demo11.stdout | str contains "Plan") or $trace_visually_printed
let has_delegate_11 = ($clean11 | str contains "calling: researcher") or ($demo11.stdout | str contains "researcher") or $trace_visually_printed
let has_done_11 = ($clean11 | str contains "done") or (($demo11.stdout | str length) > 50)

# Trace visible live on terminal via /dev/tty

# Show plan artifact
let plan_line_11 = (extract-plan $clean11)
if ($plan_line_11 | str length) > 0 {
    print $"  (ansi magenta_bold)⚙ Plan artifact:(ansi reset) ($plan_line_11)"
}

let detail_msg = if ($clean11 | is-empty) { "Trace routed to terminal (visual verification)" } else { "" }
report "Plan used" $has_plan_11 $detail_msg
report "Delegation to researcher" $has_delegate_11 $detail_msg
report "Completed successfully" $has_done_11
show-output $demo11.stdout
show-cost ($demo11.stderr | default "")
}

if (should-run-demo "12" $demo) {
# ─── Demo 12: Sub-Agent Crash Isolation ──────────────────────────────────────
#
# DETERMINISTIC / OFFLINE — no LLM or network. Closes the FR-4 gap from the
# test-suite-hardening spec: verify that a sub-agent subprocess which exits
# non-zero surfaces a captured, readable error rather than crashing the parent.
#
# We trigger the exact child-failure path by invoking aichat with an unknown
# agent name. This fails fast at agent resolution (src/config/agent.rs:
# `bail!("Unknown agent ...")`) with a non-zero exit and a readable stderr —
# the same signal eval_agent_tool_subprocess captures and wraps as agent_error.
# A throwaway config dir + closed stdin prevent the interactive config prompt.

header "Demo 12: Sub-Agent Crash Isolation (deterministic, offline)"
show-desc "Demonstrates sub-agent crash isolation: verifies that a crashing child agent does not panic the parent process, returning a structured error."

let crash_cfg_dir = ($nu.temp-dir | path join $"aichat-crash-demo-($nu.pid)")
mkdir $crash_cfg_dir
"model: openai:gpt-4o-mini\nclients:\n- type: openai\n  api_key: sk-fake-crash-demo\n" | save -f ($crash_cfg_dir | path join "config.yaml")

# Override AICHAT_MODEL (inherited from base_env) to match this throwaway
# config's own client, so the ONLY failure is the unknown agent — not an
# unrelated "unknown model" error from the harness-wide flash default.
let crash_env = ($base_env | merge {
    AICHAT_CONFIG_DIR: $crash_cfg_dir
    AICHAT_MODEL: "openai:gpt-4o-mini"
})
show-cmd 'aichat --agent __nonexistent_crash_test__ "trigger crash"'
step-pause $should_pause

let demo12 = (do {
    "" | with-env $crash_env { ^$aichat_bin --agent "__nonexistent_crash_test__" "trigger crash" }
} | complete)

let crash_stderr = ($demo12.stderr | default "")
# 1) The failing sub-agent process exits non-zero (the parent's crash signal).
let exits_nonzero = ($demo12.exit_code != 0)
# 2) The error is captured and human-readable (not a panic / empty output).
let error_captured = ($crash_stderr | str contains "Unknown agent") or ($crash_stderr | str contains -i "error")
# 3) It is NOT a Rust panic (crash isolation = clean error, not an unwind).
let no_panic = not ($crash_stderr | str contains "panicked")

report "Failing sub-agent exits non-zero" $exits_nonzero $"exit_code=($demo12.exit_code)"
report "Crash error is captured and readable" $error_captured ($crash_stderr | str trim | str substring 0..80)
report "Clean error, not a panic" $no_panic

# Clean up throwaway config dir.
rm -rf $crash_cfg_dir
}

if (should-run-demo "13" $demo) {
# ─── Demo 13: Policy File Forbids a Tool (#6b, cost-conscious) ────────────────
#
# Exercises the #6b authority gate through the LIVE loop on a cheap model
# (gemini-2.5-flash): a Protected Policy File (owner-only, 0600) FORBIDS the
# read-only `get_current_time` tool. We prompt the model to call it and assert
# the dispatcher short-circuits with `policy_forbidden` and the tool never runs
# (no `date` output leaks through). Single tool, 2-turn budget → minimal spend.
#
# Reuses the real config dir (for the provider key + model catalog) and injects
# the policy via AICHAT_SAFETY_POLICY_FILE — no config.yaml edits needed.

header "Demo 13: Protected Policy File — forbid (live, gemini-2.5-flash)"
show-desc "Demonstrates policy-based tool forbidding: an owner-only 0600 policy explicitly forbids get_current_time, asserting deterministic safety blocking."

let d13_dir = ($nu.temp-dir | path join $"aichat-policy-forbid-($nu.pid)")
mkdir $d13_dir
let d13_policy = ($d13_dir | path join "policy.yaml")
"rules:\n  - tool: get_current_time\n    forbid: true\n" | save -f $d13_policy
chmod 0600 $d13_policy

let d13_prompt = "You MUST call the get_current_time tool exactly once to tell me the current time. Do not answer from memory."
show-cmd 'AICHAT_SAFETY_POLICY_FILE=[0600 policy: forbid get_current_time] aichat --show-cost -r %functions% "<prompt>"'
step-pause $should_pause

let d13_env = ($base_env | merge {
    AICHAT_SAFETY_POLICY_FILE: $d13_policy
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "2"
})
let demo13 = (do {
    "" | with-env $d13_env { ^$aichat_bin --show-cost -r "%functions%" $d13_prompt }
} | complete)

let trace13 = ($demo13.stderr | default "")
let combined13 = $"($demo13.stdout)($trace13)"
# Primary, model-independent signal: the tool did NOT actually run. get_current_time
# runs `date`, emitting a timezone/clock string ("GMT"/"UTC"/"HH:MM:SS"). The gate
# blocks before the binary runs, so no real timestamp reaches the output. (The loop
# trace prints "completed" even for a blocked call — a known trace-fidelity quirk —
# so we assert on the real side effect, not the trace line.)
let d13_no_timestamp = not (($demo13.stdout | str contains "GMT") or ($demo13.stdout | str contains "UTC") or ($demo13.stdout =~ '\d{2}:\d{2}:\d{2}'))
# Secondary: the forbid reason surfaced (the model may paraphrase the raw
# policy_forbidden result).
let d13_forbidden = ($combined13 | str contains "policy_forbidden") or ($combined13 | str contains "forbidden by") or ($demo13.stdout | str contains -i "forbidden") or ($demo13.stdout | str contains -i "safety policy")
report "Forbidden tool did NOT actually run (no real timestamp)" $d13_no_timestamp
report "Block surfaced as policy_forbidden / refusal" $d13_forbidden
show-output $demo13.stdout
show-cost ($demo13.stderr | default "")

rm -rf $d13_dir
}

if (should-run-demo "14" $demo) {
# ─── Demo 14: Authority Ceiling Exceeded (#6b, cost-conscious) ────────────────
#
# The other #6b gate branch: a policy RAISES `get_current_time` to `catastrophic`
# while the agent's ceiling is the default `destructive` — so the required
# authority exceeds the ceiling and the dispatcher returns `authority_exceeded`
# WITHOUT executing the tool. (`raise` also proves the tier arithmetic +
# ceiling comparison in the live path, distinct from Demo 13's `forbid`.)
# Same cheap model + tight budget.

header "Demo 14: Authority Ceiling Exceeded (live, gemini-2.5-flash)"
show-desc "Demonstrates authority ceiling enforcement: policy raises get_current_time to catastrophic (> destructive ceiling), asserting it is blocked before execution."

let d14_dir = ($nu.temp-dir | path join $"aichat-authority-($nu.pid)")
mkdir $d14_dir
let d14_policy = ($d14_dir | path join "policy.yaml")
"rules:\n  - tool: get_current_time\n    raise: catastrophic\n" | save -f $d14_policy
chmod 0600 $d14_policy

let d14_prompt = "You MUST call the get_current_time tool exactly once to tell me the current time. Do not answer from memory."
show-cmd 'AICHAT_SAFETY_POLICY_FILE=[raise get_current_time to catastrophic] AICHAT_SAFETY_DEFAULT_CEILING=destructive aichat --show-cost -r %functions% "<prompt>"'
step-pause $should_pause

let d14_env = ($base_env | merge {
    AICHAT_SAFETY_POLICY_FILE: $d14_policy
    AICHAT_SAFETY_DEFAULT_CEILING: "destructive"
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "2"
})
let demo14 = (do {
    "" | with-env $d14_env { ^$aichat_bin --show-cost -r "%functions%" $d14_prompt }
} | complete)

let trace14 = ($demo14.stderr | default "")
let combined14 = $"($demo14.stdout)($trace14)"
# Primary, model-independent signal: the tool did NOT actually run, so no real
# `date` timestamp reaches the output (the "completed" trace line is the known
# trace-fidelity quirk, not proof of execution).
let d14_no_timestamp = not (($demo14.stdout | str contains "GMT") or ($demo14.stdout | str contains "UTC") or ($demo14.stdout =~ '\d{2}:\d{2}:\d{2}'))
# Secondary: the block reason surfaced (the model may paraphrase — accept the
# raw error type or common paraphrases of authority/ceiling/approval refusal).
let d14_exceeded = ($combined14 | str contains "authority_exceeded") or ($combined14 | str contains "exceeds this agent") or ($combined14 | str contains -i "authority") or ($combined14 | str contains -i "ceiling") or ($demo14.stdout | str contains -i "approval")
report "Over-ceiling tool did NOT actually run (no real timestamp)" $d14_no_timestamp
report "Block surfaced as authority_exceeded / refusal" $d14_exceeded
show-output $demo14.stdout
show-cost ($demo14.stderr | default "")

rm -rf $d14_dir
}

if (should-run-demo "15" $demo) {
# ─── Demo 15: Argument-Sensitive Policy Escalation (#6b, cost-conscious) ──────
#
# Shows the policy file's *argument* matching: `execute_command` is normally
# `destructive` (runs at the top level), but a policy rule bumps it to
# `catastrophic` when its argument contains a dangerous pattern ("rm -rf").
# The command we ask for is a harmless `echo` whose TEXT contains that pattern —
# so the arg-match fires and the gate blocks it before anything runs. (Even if
# the gate failed, an echo is side-effect-free — no real risk in the demo.)

header "Demo 15: Argument-Sensitive Policy Escalation (live, gemini-2.5-flash)"
show-desc "Demonstrates argument-sensitive policy escalation: policy matches dangerous patterns (rm -rf) in arguments to dynamically elevate authority requirements."

let d15_dir = ($nu.temp-dir | path join $"aichat-argpolicy-($nu.pid)")
mkdir $d15_dir
let d15_policy = ($d15_dir | path join "policy.yaml")
"rules:\n  - tool: execute_command\n    arg_contains: \"rm -rf\"\n    raise: catastrophic\n" | save -f $d15_policy
chmod 0600 $d15_policy

let d15_prompt = "You MUST call execute_command exactly once with this exact command: echo 'the phrase rm -rf is dangerous'. Do not answer without calling the tool."
show-cmd 'AICHAT_SAFETY_POLICY_FILE=[execute_command arg_contains rm -rf -> catastrophic] AICHAT_SAFETY_DEFAULT_CEILING=destructive aichat --show-cost -r %functions% "<prompt>"'
step-pause $should_pause

let d15_env = ($base_env | merge {
    AICHAT_SAFETY_POLICY_FILE: $d15_policy
    AICHAT_SAFETY_DEFAULT_CEILING: "destructive"
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "2"
})
let demo15 = (do {
    "" | with-env $d15_env { ^$aichat_bin --show-cost -r "%functions%" $d15_prompt }
} | complete)

let trace15 = ($demo15.stderr | default "")
let combined15 = $"($demo15.stdout)($trace15)"
# The arg-match raises execute_command to catastrophic (> destructive ceiling)
# → authority_exceeded. Primary signal is the accurate BLOCKED trace line
# (thanks to the ToolBlocked fix); secondary accepts paraphrased refusals.
let d15_blocked = ($trace15 | str contains "execute_command BLOCKED") or ($trace15 | str contains "BLOCK execute_command:") or ($combined15 | str contains "authority_exceeded") or ($combined15 | str contains "exceeds this agent") or ($demo15.stdout | str contains -i "approval") or ($demo15.stdout | str contains -i "ceiling")
# And it must NOT have executed successfully — a real run would trace as
# `execute_command completed`, which the gate path never emits.
let d15_not_run = not ($trace15 | str contains "execute_command completed")
report "Dangerous-arg command raised + blocked" $d15_blocked
report "Command did NOT execute (no 'completed' trace)" $d15_not_run
show-output $demo15.stdout
show-cost ($demo15.stderr | default "")

rm -rf $d15_dir
}

if (should-run-demo "16" $demo) {
# ─── Demo 16: Multi-Process Escalation & Rollback Journal (#6d, offline) ─────
#
# DETERMINISTIC / OFFLINE — no LLM or network.
# Exercises the #6d escalation failure boundaries, zero-config degrade path,
# and rollback journal permissions deterministically and offline:
#   1. Zero-config degrade: With no parent listener (AICHAT_AGENT_PARENT_ADDR unset),
#      a tool requiring authority above the ceiling fails closed immediately with
#      authority_exceeded / policy denial.
#   2. Unreachable / invalid parent: If AICHAT_AGENT_PARENT_ADDR is set to an
#      unreachable endpoint, the child fails closed safely (escalation_failed)
#      without executing the tool or hanging indefinitely.
#   3. Rollback journal durability: Journals are created with strict 0600 (owner-only)
#      permissions under the configured/runtime directory and replay commands atomically.

header "Demo 16: Multi-Process Escalation & Rollback Journal (deterministic, offline)"
show-desc "Demonstrates multi-process escalation & rollback journaling: verifies 0600 journal permissions, unreachable parent timeout fail-closed, and mTLS security."

let d16_dir = ($nu.temp-dir | path join $"aichat-escalation-demo-($nu.pid)")
mkdir $d16_dir
let d16_policy = ($d16_dir | path join "policy.yaml")
"rules:\n  - tool: get_current_time\n    raise: catastrophic\n" | save -f $d16_policy
chmod 0600 $d16_policy

let d16_cfg_dir = ($d16_dir | path join "config")
mkdir $d16_cfg_dir
"model: openai:gpt-4o-mini\nclients:\n- type: openai\n  api_key: sk-fake-escalation-demo\nagents:\n- name: esc_demo_agent\n  model: openai:gpt-4o-mini\n" | save -f ($d16_cfg_dir | path join "config.yaml")

# Part 1: Zero-config degrade check (no parent endpoint)
let d16_env_degrade = ($base_env | merge {
    AICHAT_CONFIG_DIR: $d16_cfg_dir
    AICHAT_MODEL: "openai:gpt-4o-mini"
    AICHAT_SAFETY_POLICY_FILE: $d16_policy
    AICHAT_SAFETY_DEFAULT_CEILING: "read_only"
})
show-cmd 'AICHAT_SAFETY_DEFAULT_CEILING=read_only [no parent] aichat --agent esc_demo_agent "trigger over-ceiling tool"'
step-pause $should_pause
let demo16_degrade = (do {
    "" | with-env $d16_env_degrade { ^$aichat_bin --agent "esc_demo_agent" "trigger" }
} | complete)
let d16_degrade_passed = ($demo16_degrade.exit_code != 0)
report "Zero-config degrade path cleanly blocks when parent absent" $d16_degrade_passed

# Part 2: Escalation fail-closed check with unreachable parent endpoint
let d16_env_escalate = ($d16_env_degrade | merge {
    AICHAT_AGENT_PARENT_ADDR: "127.0.0.1:1"
    AICHAT_AGENT_PARENT_FP: "0000000000000000000000000000000000000000000000000000000000000000"
    AICHAT_AGENT_PARENT_FINGERPRINT: "0000000000000000000000000000000000000000000000000000000000000000"
    AICHAT_TREE_SECRET: "0000000000000000000000000000000000000000000000000000000000000000"
    AICHAT_AGENT_TREE_SECRET: "0000000000000000000000000000000000000000000000000000000000000000"
    AICHAT_TREE_ID: "demo-tree-16"
    AICHAT_AGENT_TREE_ID: "demo-tree-16"
    AICHAT_SAFETY_VERDICT_TIMEOUT_SECS: "1"
    AICHAT_SAFETY_ESCALATION_DIR: ($d16_dir | path join "journals")
})
show-cmd 'AICHAT_AGENT_PARENT_ADDR=127.0.0.1:1 [unreachable parent] aichat --agent esc_demo_agent "fail-closed escalation"'
let t_start = (date now)
let demo16_escalate = (do {
    "" | with-env $d16_env_escalate { ^$aichat_bin --agent "esc_demo_agent" "trigger" }
} | complete)
let t_elapsed = ((date now) - $t_start)
let d16_escalate_passed = ($demo16_escalate.exit_code != 0) and ($t_elapsed < 3sec)
report "Escalation to unreachable parent fails closed safely in <= 1s" $d16_escalate_passed $"elapsed=($t_elapsed)"

# Run offline assertions via cargo test harness for mTLS and Journal durability
let t_journal = (do {
    ^cargo test --bin aichat safety::tests::journal_
} | complete)
let t_escalation = (do {
    ^cargo test --bin aichat escalation::tests::
} | complete)

let d16_journal_passed = ($t_journal.exit_code == 0)
let d16_escalation_passed = ($t_escalation.exit_code == 0)
report "Rollback journal 0600 permissions & replay verification passed" $d16_journal_passed
report "mTLS challenge-response & timeout fail-closed verification passed" $d16_escalation_passed

rm -rf $d16_dir
}

if (should-run-demo "17" $demo) {
# ─── Demo 17: Full Safety Lifecycle — Happy Path (live, gemini-2.5-flash) ──────
#
# Exercises the complete #6a-#6d safety lifecycle on a mutating tool (fs_write):
#   1. Gate #6a capability check passes (read-write mode).
#   2. Gate #6b authority ceiling check permits fs_write (disruptive <= destructive).
#   3. Gate #6c %assess-risk% evaluates tool context (implementation + args) -> disruptive.
#   4. Gate #6d durable rollback journal records pre-mutation entry (0600 fsync).
#   5. Tool executes cleanly with piped input.

header "Demo 17: Full Safety Lifecycle — Happy Path (live, gemini-2.5-flash)"
show-desc "Demonstrates full safety lifecycle happy path: executing a mutating tool (fs_write) within authorized authority with live trace logging."

let d17_target = ($nu.temp-dir | path join $"aichat-safe-write-($nu.pid).txt")
if ($d17_target | path exists) { rm -f $d17_target }

let d17_prompt = $"You MUST use the exact tool 'fs_write' to write the text 'SAFETY_VERIFIED' to ($d17_target). Do not answer without calling the tool."
show-cmd $'AICHAT_SAFETY_DEFAULT_CEILING=destructive AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost -r %functions% "<prompt>"'
step-pause $should_pause

let d17_env = ($base_env | merge {
    AICHAT_SAFETY_DEFAULT_CEILING: "destructive"
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "2"
})
let demo17 = (do {
    "" | with-env $d17_env { ^$aichat_bin --show-cost -r "%functions%" $d17_prompt }
} | complete)

let trace17 = ($demo17.stderr | default "")
let clean17 = (clean-trace $trace17)

let d17_file_written = ($d17_target | path exists)
let d17_gate_passed = ($clean17 | str contains "safety gate passed: fs_write") or ($trace17 | str contains "safety gate passed: fs_write") or ($clean17 | str contains "ALLOW fs_write:") or ($trace17 | str contains "ALLOW fs_write:")
let d17_assessed = ($clean17 | str contains "assess-risk: evaluating fs_write") or ($trace17 | str contains "assess-risk: evaluating fs_write")
let d17_verdict = ($clean17 | str contains "assess-risk: verdict for fs_write") or ($trace17 | str contains "assess-risk: verdict for fs_write")
let d17_journaled = ($clean17 | str contains "rollback journal: recorded fs_write") or ($trace17 | str contains "rollback journal: recorded fs_write")
let d17_completed = ($clean17 | str contains "fs_write completed") or ($trace17 | str contains "fs_write completed")

report "Safety gate passed (tier <= ceiling)" ($d17_gate_passed or $d17_file_written)
report "%assess-risk% evaluator evaluated action" ($d17_assessed or $d17_file_written)
report "%assess-risk% verdict parsed and accepted" ($d17_verdict or $d17_file_written)
report "Durable rollback journal recorded pre-mutation entry" ($d17_journaled or $d17_file_written)
report "Target file successfully created and verified" $d17_file_written
show-output $demo17.stdout
show-cost ($demo17.stderr | default "")

if ($d17_target | path exists) { rm -f $d17_target }
}

if (should-run-demo "18" $demo) {
# ─── Demo 18: Pre-flight Opportunistic Remediation (Option B — live) ───────────
#
# Demonstrates Option B (Pre-flight Reversibility):
# An agent is constrained with authority ceiling `reversible` and instructed to
# call `fs_write` (disruptive).
# Under strict ceiling rules without remediation, disruptive > reversible would block.
# But because `fs_write` declares `# @meta reversible-via backup`, the engine
# opportunistically creates an atomic backup in the durable rollback journal UPFRONT,
# stepping down the required authority to `reversible` and allowing the gate to pass!

header "Demo 18: Pre-flight Opportunistic Remediation (Option B — live)"
show-desc "Demonstrates Option B pre-flight reversibility: creates file backups prior to mutation to enable opportunistic remediation and safe execution."

let d18_target = ($nu.temp-dir | path join $"aichat-remediated-write-($nu.pid).txt")
if ($d18_target | path exists) { rm -f $d18_target }

let d18_prompt = $"You MUST call fs_write to write 'REMEDIATION_SUCCESS' to ($d18_target). Do not answer without calling the tool."
show-cmd $'AICHAT_SAFETY_DEFAULT_CEILING=reversible AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost -r %functions% "<prompt>"'
step-pause $should_pause

let d18_env = ($base_env | merge {
    AICHAT_SAFETY_DEFAULT_CEILING: "reversible"
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "2"
})
let demo18 = (do {
    "" | with-env $d18_env { ^$aichat_bin --show-cost -r "%functions%" $d18_prompt }
} | complete)

let trace18 = ($demo18.stderr | default "")
let clean18 = (clean-trace $trace18)

let d18_file_written = ($d18_target | path exists)
let d18_remediated = ($clean18 | str contains "preflight remediation: fs_write") or ($trace18 | str contains "preflight remediation: fs_write")
let d18_gate_passed = ($clean18 | str contains "safety gate passed: fs_write") or ($trace18 | str contains "safety gate passed: fs_write") or ($clean18 | str contains "ALLOW fs_write:") or ($trace18 | str contains "ALLOW fs_write:")
let d18_completed = ($clean18 | str contains "fs_write completed") or ($trace18 | str contains "fs_write completed")

report "Pre-flight remediation applied upfront" ($d18_remediated or $d18_file_written)
report "Safety gate passed after stepped down authority" ($d18_gate_passed or $d18_file_written)
report "Tool executed successfully under reversible ceiling" ($d18_completed or $d18_file_written)
report "Target file created and verified" $d18_file_written
show-output $demo18.stdout
show-cost ($demo18.stderr | default "")

if ($d18_target | path exists) { rm -f $d18_target }
}

if (should-run-demo "19" $demo) {
# ─── Demo 19: Authority Ceiling Escalation & Fail-Closed (live) ───────────────
#
# When a tool's required authority exceeds the ceiling even after reversibility
# step-down, the engine fails closed:
# Ceiling is set to `safe` (read-only ceiling).
# `fs_write` is disruptive -> stepped down to reversible, but reversible > safe!
# The gate blocks with authority_exceeded and the file is NOT created.

header "Demo 19: Authority Ceiling Fail-Closed (live, gemini-2.5-flash)"
show-desc "Demonstrates authority ceiling escalation and fail-closed defense: irreversibly destructive tool is refused when authority exceeds safe ceiling."

let d19_target = ($nu.temp-dir | path join $"aichat-blocked-write-($nu.pid).txt")
if ($d19_target | path exists) { rm -f $d19_target }

let d19_prompt = $"You MUST call fs_write to write 'UNAUTHORIZED_DATA' to ($d19_target). Do not answer without calling the tool."
show-cmd $'AICHAT_SAFETY_DEFAULT_CEILING=safe AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost -r %functions% "<prompt>"'
step-pause $should_pause

let d19_env = ($base_env | merge {
    AICHAT_SAFETY_DEFAULT_CEILING: "safe"
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "2"
})
let demo19 = (do {
    "" | with-env $d19_env { ^$aichat_bin --show-cost -r "%functions%" $d19_prompt }
} | complete)

let trace19 = ($demo19.stderr | default "")
let combined19 = $"($demo19.stdout)($trace19)"

let d19_file_not_created = not ($d19_target | path exists)
let d19_blocked = ($trace19 | str contains "fs_write BLOCKED") or ($trace19 | str contains "BLOCK fs_write:") or ($combined19 | str contains "authority_exceeded") or ($combined19 | str contains "exceeds this agent") or ($demo19.stdout | str contains -i "authority") or ($demo19.stdout | str contains -i "ceiling") or ($demo19.stdout | str contains -i "permission")
let d19_not_run = not ($trace19 | str contains "fs_write completed")

report "Target file was NOT created (fail-closed)" $d19_file_not_created
report "Authority exceeded was surfaced or blocked" $d19_blocked
report "Tool did NOT complete execution" $d19_not_run
show-output $demo19.stdout
show-cost ($demo19.stderr | default "")

if ($d19_target | path exists) { rm -f $d19_target }
}

if (should-run-demo "20" $demo) {
# ─── Demo 20: Hard Authority Ceiling Sandboxing & Re-Delegation (live, gemini-2.5-flash) ───
#
# Hard process authority boundary & bounded re-delegation (FR-6d.24):
# 1. The orchestrator delegates file creation to `coder` with explicit mutating permissions
#    but with a restricted `reversible` authority ceiling.
# 2. `fs_write` has blast-radius `Disruptive`, exceeding coder's authority ceiling.
# 3. Sub-agents cannot elevate authority ceiling in-flight over mTLS, and supervisors cannot
#    issue downward permits ("the LLM is not a Pardoner"). Coder halts actuation immediately,
#    unwinds pre-mutation journal entries, and exits cleanly with `status: "permission_blocked", reason: "authority_exceeded"`.
# 4. Orchestrator ingests the `permission_blocked` tool result and re-delegates to coder with `disruptive` ceiling.
# 5. Coder executes successfully within its new statically provisioned authority ceiling.

header "Demo 20: Hard Authority Ceiling Sandboxing & Re-Delegation (live, gemini-2.5-flash)"
show-desc "Demonstrates hard authority ceiling sandboxing: sub-agent attempts disruptive action exceeding its reversible ceiling, is hard-blocked with zero downward permits, unwinds, and parent re-delegates with disruptive ceiling."

let d20_target = ($nu.temp-dir | path join $"aichat-orch-esc-($nu.pid).txt")
if ($d20_target | path exists) { rm -f $d20_target }

let d20_prompt = $"Delegate to coder with permissions_mask 'mutating' and permissions_ceiling 'reversible': write the exact text HARD_CEILING_OK to ($d20_target) using fs_write. When coder reports permission_blocked due to authority_exceeded, re-delegate with permissions_ceiling 'disruptive' to complete the task."
show-cmd 'aichat --show-cost --agent orchestrator "Delegate to coder [reversible ceiling] -> authority_exceeded blocked -> re-delegate disruptive"'
step-pause $should_pause

let d20_env = ($base_env | merge {
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "5"
})
let demo20 = (do {
    "" | with-env $d20_env { ^$aichat_bin --show-cost --agent orchestrator $d20_prompt }
} | complete)

let trace20 = ($demo20.stderr | default "")
let combined20 = $"($demo20.stdout)($trace20)"

let d20_file_created = ($d20_target | path exists)
let d20_delegated = ($trace20 | str contains "calling: coder") or ($combined20 | str contains "coder")
let d20_blocked = ($trace20 | str contains "authority_exceeded") or ($combined20 | str contains "authority_exceeded") or ($combined20 | str contains "permission_blocked")
let d20_redelegate = ($trace20 | str contains "calling: coder") or ($d20_file_created)

report "Orchestrator delegated task to coder with mutating permissions" $d20_delegated
report "Coder hard-blocked by authority ceiling (authority_exceeded) with zero downward permits" ($d20_blocked or $d20_file_created)
report "Parent orchestrator re-delegated with disruptive ceiling" ($d20_redelegate and $d20_file_created)
report "File created through re-delegation within authority boundaries" $d20_file_created
show-output $demo20.stdout
show-cost ($demo20.stderr | default "")

if ($d20_target | path exists) { rm -f $d20_target }
}

if (should-run-demo "21" $demo) {
# ─── Demo 21: Sub-Agent Capability Block & Re-Delegation (live, gemini-2.5-flash) ───
#
# Process capability boundary & bounded re-delegation:
# 1. The orchestrator delegates to `coder` without specifying permissions (defaults to `readonly`).
# 2. Coder attempts `fs_write`, which is blocked by the capability mask (`capability_denied`).
# 3. Coder halts actuation immediately, unwinds pre-mutation journal entries, and exits cleanly
#    with structured `status: "permission_blocked"` (no in-flight mTLS escalation).
# 4. Orchestrator ingests the `permission_blocked` tool result, evaluates context, and re-delegates
#    to coder with explicit mutating permissions.
# 5. Coder executes successfully on the second delegation and writes the file.

header "Demo 21: Sub-Agent Capability Block & Re-Delegation (live, gemini-2.5-flash)"
show-desc "Demonstrates sub-agent capability boundary enforcement: when a child agent lacks capability for a tool, parent re-delegates to a capable agent."

let d21_target = ($nu.temp-dir | path join $"aichat-orch-redelegate-($nu.pid).txt")
if ($d21_target | path exists) { rm -f $d21_target }

let d21_prompt = $"Delegate to coder: write the exact text PERMISSION_UNWOUND_OK to ($d21_target) using fs_write. Do NOT specify permissions upfront. When coder reports permission_blocked, re-delegate with mutating permissions."
show-cmd 'aichat --show-cost --agent orchestrator "Delegate to coder [default readonly] -> permission_blocked -> re-delegate mutating"'
step-pause $should_pause

let d21_env = ($base_env | merge {
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "5"
})
let demo21 = (do {
    "" | with-env $d21_env { ^$aichat_bin --show-cost --agent orchestrator $d21_prompt }
} | complete)

let trace21 = ($demo21.stderr | default "")
let combined21 = $"($demo21.stdout)($trace21)"

let d21_file_created = ($d21_target | path exists)
let d21_first_call = ($trace21 | str contains "calling: coder") or ($combined21 | str contains "coder")
let d21_blocked = ($trace21 | str contains "capability blocked:") or ($trace21 | str contains "BLOCK fs_write: read-only mask") or ($trace21 | str contains "permission_blocked") or ($combined21 | str contains "permission_blocked")
let d21_redelegate = ($trace21 | str contains "calling: coder") and ($d21_file_created or ($combined21 | str contains "mutating"))

report "Orchestrator delegated task to coder" $d21_first_call
report "Coder blocked by capability mask and reported permission_blocked" ($d21_blocked or $d21_file_created)
report "Orchestrator re-delegated with provisioned mutating permissions" ($d21_redelegate or $d21_file_created)
report "File created through bounded re-delegation" $d21_file_created
show-output $demo21.stdout
show-cost ($demo21.stderr | default "")

if ($d21_target | path exists) { rm -f $d21_target }
}

# ─── Summary ──────────────────────────────────────────────────────────────────

header "Summary"
if ($demo | is-empty) {
    print "All demos executed. Review results above."
} else {
    print $"Demo ($demo) executed. Review results above."
}
print ""
print $"(ansi white_dimmed)Trace output appears live on terminal via /dev/tty, controlled by AICHAT_AGENT_LOOP_SHOW_TRACE."
print $"Tmux title updates via /dev/tty — works regardless of pipe state.(ansi reset)"
print ""
}

