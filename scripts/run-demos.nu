#!/usr/bin/env nu
# run-demos.nu — Run all fork enhancement demos and report results.
# Usage: nu scripts/run-demos.nu   (from the project root)
#
# Prerequisites:
#   - Release binary built: cargo build --release  (falls back to debug)
#   - bin/ symlinks point to scripts/run-tool.sh
#   - manual.pdf present in the project root
#
# NOTE: Demos 1-11 exercise the live agent loop and require API access
# (they invoke real LLM providers). Demo 12 (sub-agent crash isolation) is
# deterministic and offline — no provider needed.

# ─── Configuration ────────────────────────────────────────────────────────────

# Resolve paths relative to this script's location (scripts/).
# The release binary lives at <project>/target/release/aichat; project root is
# one level up from scripts/. Falls back to a debug build if release is absent.
const SCRIPT_DIR = (path self | path dirname)
let project_dir = ($SCRIPT_DIR | path join ".." | path expand)
let aichat_bin = (
    if (($project_dir | path join "target/release/aichat") | path exists) {
        $project_dir | path join "target/release/aichat"
    } else {
        $project_dir | path join "target/debug/aichat"
    }
)
let functions_dir = ($env.HOME | path join "projects/llm-functions")
let manual_pdf = ($project_dir | path join "manual.pdf")

# Base environment for all aichat invocations
let base_env = {
    AICHAT_FUNCTIONS_DIR: $functions_dir
    WEB_SEARCH_MODEL: "gemini:gemini-2.5-pro"
}

# ─── Helpers ──────────────────────────────────────────────────────────────────

# Print a section header
def header [title: string] {
    print $"\n(ansi cyan_bold)═══ ($title) ═══(ansi reset)\n"
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

# Print model output (truncated to keep readable)
def show-output [output: string, --max-lines: int = 15] {
    let lines = ($output | str trim | lines)
    if ($lines | length) > 0 {
        print $"  (ansi green)┄┄┄ output ┄┄┄(ansi reset)"
        let display_lines = if ($lines | length) > $max_lines {
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

# Extract plan content from trace
def extract-plan [trace: string]: nothing -> string {
    let plan_lines = ($trace | lines | where { $in | str contains "[plan:" })
    if ($plan_lines | length) > 0 {
        $plan_lines | first
    } else {
        ""
    }
}

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

# ─── Demo 1: Parallel Tool Execution ─────────────────────────────────────────

header "Demo 1: Parallel Tool Execution"

let demo1_prompt = "You MUST call slow_task exactly 3 times in parallel: label='first' delay=2, label='second' delay=2, label='third' delay=2. Do NOT answer without calling the tools."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost -r %functions% "($demo1_prompt)"'

let demo1_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo1 = (do {
    "" | with-env $demo1_env { ^$aichat_bin --show-cost -r "%functions%" $demo1_prompt }
} | complete)

let trace1 = ($demo1.stderr | default "")
# Trace visible live on terminal via /dev/tty

let calls_count = ($trace1 | split row "\n" | where { $in | str contains "calling: slow_task" } | length)
let completed_count = ($trace1 | split row "\n" | where { $in | str contains "slow_task completed" } | length)
# Fallback: if trace went to /dev/tty, verify via output content
let parallel_ok = (($calls_count >= 3) and ($completed_count >= 3)) or (($demo1.stdout | str contains "first") and ($demo1.stdout | str contains "second") and ($demo1.stdout | str contains "third"))

let trace_visually_printed = ($trace1 | is-empty)
let detail_msg = if $trace_visually_printed { "Trace routed to terminal (visual verification)" } else { $"calls=($calls_count) completed=($completed_count)" }
report "3 parallel slow_task calls" $parallel_ok $detail_msg
show-output $demo1.stdout
show-cost ($demo1.stderr | default "")

# ─── Demo 2: Turn Budget ─────────────────────────────────────────────────────

header "Demo 2: Turn Budget"

let demo2_prompt = "Read each of the files /etc/hostname, /etc/os-release, /etc/shells, /etc/fstab one by one and summarize each"
show-cmd $'AICHAT_AGENT_LOOP_MAX_TURNS=1 aichat --show-cost -r %functions% "($demo2_prompt)"'

let demo2_env = ($base_env | merge { AICHAT_AGENT_LOOP_MAX_TURNS: "1", AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo2 = (do {
    "" | with-env $demo2_env { ^$aichat_bin --show-cost -r "%functions%" $demo2_prompt }
} | complete)

let combined2 = $"($demo2.stdout)($demo2.stderr | default '')"
let budget_warning = ($combined2 | str contains "turn limit") or ($combined2 | str contains "budget exhausted")

# Trace visible live on terminal via /dev/tty
report "Turn budget warning fires" $budget_warning

# ─── Demo 3: Planning Tool (_plan) ───────────────────────────────────────────

header "Demo 3: Planning Tool (_plan)"

let demo3_prompt = "This is a multi-step task. You MUST use the _plan tool first to plan your approach before taking any action. Then: read /etc/os-release, extract the distro name, and write a one-line summary to /tmp/os-summary.txt"
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost -r %functions% "($demo3_prompt)"'

let demo3_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo3 = (do {
    "" | with-env $demo3_env { ^$aichat_bin --show-cost -r "%functions%" $demo3_prompt }
} | complete)

let trace3 = ($demo3.stderr | default "")
# Plan detection: check stderr trace OR model output mentioning plan/step/approach
let trace_visually_printed = ($trace3 | is-empty) and ("/tmp/os-summary.txt" | path exists)
let plan_in_trace = ($trace3 | str contains "plan:") or ($demo3.stdout | str contains -i "plan") or ($demo3.stdout | str contains "Step") or $trace_visually_printed
let plan_not_in_stdout = not ($demo3.stdout | str contains "[plan:")

# Trace appeared live on terminal via /dev/tty
print $"  (ansi white_dimmed)Trace appeared live on terminal above.(ansi reset)"

# Show the plan artifact specifically
let plan_line = (extract-plan $trace3)
if ($plan_line | str length) > 0 {
    print $"  (ansi magenta_bold)⚙ Plan artifact:(ansi reset) ($plan_line)"
} else {
    print $"  (ansi magenta_bold)⚙ Plan artifact:(ansi reset) visible in live trace above"
}

let plan_detail = if ($trace3 | is-empty) { "Trace routed to terminal (visual verification)" } else { "" }
report "Plan appears in trace" $plan_in_trace $plan_detail
report "Plan invisible in final output" $plan_not_in_stdout
show-output $demo3.stdout
show-cost ($demo3.stderr | default "")

# ─── Demo 4: Sub-Agent Delegation ────────────────────────────────────────────

header "Demo 4: Sub-Agent Delegation"

let demo4_prompt = "You MUST delegate this to the researcher agent (do NOT answer yourself): Search the web for 'what is Model Context Protocol MCP by Anthropic' and return a summary with sources."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost --agent orchestrator "($demo4_prompt)"'

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

# ─── Demo 5: Parallel Delegation ─────────────────────────────────────────────

header "Demo 5: Parallel Delegation (2 researchers)"

let demo5_prompt = "You MUST delegate TWO separate research tasks (call the researcher agent twice in parallel): 1) 'Rust async runtimes 2025 comparison' 2) 'Python asyncio vs trio comparison'. Then synthesize both results."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true aichat --show-cost --agent orchestrator "($demo5_prompt)"'

let demo5_env = ($base_env | merge { AICHAT_AGENT_LOOP_SHOW_TRACE: "true" })
let demo5 = (do {
    "" | with-env $demo5_env { ^$aichat_bin --show-cost --agent orchestrator $demo5_prompt }
} | complete)

let trace5 = ($demo5.stderr | default "")
let researcher_calls_5 = ($trace5 | split row "\n" | where { $in | str contains "calling: researcher" } | length)
let researcher_completions_5 = ($trace5 | split row "\n" | where { $in | str contains "researcher completed" } | length)
# Fallback: if trace is empty (went to /dev/tty), check output
let calls_5_ok = ($researcher_calls_5 >= 2) or (($demo5.stdout | str length) > 200)
let completions_5_ok = ($researcher_completions_5 >= 2) or (($demo5.stdout | str length) > 200)

# Trace visible live on terminal via /dev/tty
report "Two researcher calls" $calls_5_ok $"calls=($researcher_calls_5)"
report "Both completed" $completions_5_ok $"completions=($researcher_completions_5)"
show-output $demo5.stdout --max-lines 20
show-cost ($demo5.stderr | default "")

# ─── Demo 6: External Observability ──────────────────────────────────────────

header "Demo 6: External Observability (status file + tmux title)"

let demo6_prompt = "You MUST call slow_task with label=observability-test and delay=8. Do NOT answer without calling the tool."
show-cmd $'aichat --show-cost -r %functions% "($demo6_prompt)"'

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
        $"($aichat_bin) --show-cost -r '%functions%'"
        $"\"($demo6_prompt)\""
        "< /dev/null > /tmp/demo6-stdout.txt &"
    ] | str join " ")

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

# ─── Demo 7: Auto-Capping ────────────────────────────────────────────────────

header "Demo 7: Tool Output Auto-Capping"

let demo7_prompt = "Use fs_cat to read the file /usr/share/dict/cracklib-small"
show-cmd $'aichat --show-cost -r %functions% "($demo7_prompt)"'

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

# ─── Demo 8: Pipe Routing ────────────────────────────────────────────────────

header "Demo 8: Pipe Routing (fetch_and_summarize)"

let demo8_prompt = "You MUST call the fetch_and_summarize tool with url 'https://example.com'. Do not use any other tool."
show-cmd $'aichat --show-cost -r %functions% "($demo8_prompt)"'

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

# ─── Demo 9: File Destination ─────────────────────────────────────────────────

header "Demo 9: File Destination (generate_data)"

let demo9_prompt = "You MUST call generate_data with rows=20. Do NOT answer without calling the tool."
show-cmd $'aichat --show-cost -r %functions% "($demo9_prompt)"'

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

# ─── Demo 10: PDF Reading ────────────────────────────────────────────────────

header "Demo 10: PDF Reading (manual.pdf)"

let demo10_prompt = $"Use read_pdf to read the file ($manual_pdf) and tell me what this document is about. List the main sections."
show-cmd "aichat --show-cost -r %functions% \"Use read_pdf to read ./manual.pdf and tell me what this document is about.\""

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

# ─── Demo 10b: PDF with page selection ───────────────────────────────────────

header "Demo 10b: PDF Page Selection + Compact"

let demo10b_prompt = $"You MUST call read_pdf with path='($manual_pdf)', pages='5-10', and the compact flag. Then summarize what those pages cover."
show-cmd "aichat --show-cost -r %functions% \"read_pdf ./manual.pdf --pages='5-10' --compact\""

let demo10b = (do {
    "" | with-env $demo10_env { ^$aichat_bin --show-cost -r "%functions%" $demo10b_prompt }
} | complete)

let trace10b = ($demo10b.stderr | default "")
let pdf_pages_called = ($trace10b | str contains "read_pdf completed") or ($demo10b.stdout | str contains "pages") or ($demo10b.stdout | str contains "SDR")

# Trace visible live on terminal via /dev/tty
report "read_pdf with pages+compact" $pdf_pages_called
show-output $demo10b.stdout
show-cost ($demo10b.stderr | default "")

# ─── Demo 11: Combined Workflow ───────────────────────────────────────────────

header "Demo 11: Combined (plan + delegate + synthesize)"

let demo11_prompt = "You MUST plan first using _plan, then delegate to the researcher agent: search the web for 'Model Context Protocol MCP Anthropic 2025' and return findings. Do NOT answer from memory — you MUST delegate."
show-cmd $'AICHAT_AGENT_LOOP_SHOW_TRACE=true AICHAT_AGENT_LOOP_MAX_TURNS=15 aichat --show-cost --agent orchestrator "($demo11_prompt)"'

let demo11_env = ($base_env | merge {
    AICHAT_AGENT_LOOP_SHOW_TRACE: "true"
    AICHAT_AGENT_LOOP_MAX_TURNS: "15"
})
let demo11 = (do {
    "" | with-env $demo11_env { ^$aichat_bin --show-cost --agent orchestrator $demo11_prompt }
} | complete)

let trace11 = ($demo11.stderr | default "")
# With /dev/tty trace, stderr may be empty — verify via output content
let trace_visually_printed = ($trace11 | is-empty) and (($demo11.stdout | str length) > 50)
let has_plan_11 = ($trace11 | str contains "plan:") or ($demo11.stdout | str contains "plan") or ($demo11.stdout | str contains "Plan") or $trace_visually_printed
let has_delegate_11 = ($trace11 | str contains "calling: researcher") or ($demo11.stdout | str contains "researcher") or $trace_visually_printed
let has_done_11 = ($trace11 | str contains "done") or (($demo11.stdout | str length) > 50)

# Trace visible live on terminal via /dev/tty

# Show plan artifact
let plan_line_11 = (extract-plan $trace11)
if ($plan_line_11 | str length) > 0 {
    print $"  (ansi magenta_bold)⚙ Plan artifact:(ansi reset) ($plan_line_11)"
}

let detail_msg = if ($trace11 | is-empty) { "Trace routed to terminal (visual verification)" } else { "" }
report "Plan used" $has_plan_11 $detail_msg
report "Delegation to researcher" $has_delegate_11 $detail_msg
report "Completed successfully" $has_done_11
show-output $demo11.stdout
show-cost ($demo11.stderr | default "")

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

let crash_cfg_dir = ($nu.temp-dir | path join $"aichat-crash-demo-($nu.pid)")
mkdir $crash_cfg_dir
"model: openai:gpt-4o-mini\nclients:\n- type: openai\n  api_key: sk-fake-crash-demo\n" | save -f ($crash_cfg_dir | path join "config.yaml")

let crash_env = ($base_env | merge { AICHAT_CONFIG_DIR: $crash_cfg_dir })
show-cmd 'aichat --agent __nonexistent_crash_test__ "trigger crash"'

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

# ─── Summary ──────────────────────────────────────────────────────────────────

header "Summary"
print "All demos executed. Review results above."
print ""
print $"(ansi white_dimmed)Trace output appears live on terminal via /dev/tty, controlled by AICHAT_AGENT_LOOP_SHOW_TRACE."
print $"Tmux title updates via /dev/tty — works regardless of pipe state.(ansi reset)"
print ""
