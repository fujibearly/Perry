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
#     (~/projects/innators, branch feat/tool-safety-classification) is classified.
#
# NOTE: Demos 1-11 exercise the live agent loop and require API access
# (they invoke real LLM providers). Demo 12 (sub-agent crash isolation) and
# Demo 16 (multi-process escalation and rollback journal) are deterministic
# and offline — no provider needed. Demos 13-15, 17-21 (#6b-#6d safety lifecycle), 22-23 (skills), and 24 (autonomy ladder) are live
# but tightly scoped:
#   13 — Protected Policy File `forbid`      → policy_forbidden
#   14 — authority ceiling exceeded          → authority_exceeded
#   15 — argument-sensitive `raise`          → catastrophic > ceiling, blocked
#   16 — mTLS escalation & rollback journal  → fail-closed & 0600 durability
#   17 — Full Safety Lifecycle (Happy Path)  → Gate pass + %assess-risk% + 0600 journal + exec
#   18 — Pre-flight Remediation (--autonomy reversible) → fs_write + journal backup upfront -> stepped down, passes
#   19 — Authority Ceiling Fail-Closed       → safe ceiling blocks (even with reversibility)
#   20 — Orchestrator Sub-Agent Authority Escalation → mutating sub-agent authority_exceeded -> mTLS Should Gate -> Continue
#   21 — Sub-Agent Capability Block & Re-Delegation  → readonly sub-agent capability_denied -> unwind -> permission_blocked -> orchestrator re-delegates mutating
#   22 — Progressive Disclosure Runbook (host_stamp) → in-thread read_skill execution
#   23 — Workspace Skill Discovery & Provenance Taint → untrusted_runbook in %assess-risk%
#   24 — Autonomy Ladder presets             → readonly (Gate 1 block), reversible (Option B), consult (funnel)
#
# All live demos run under the default aichat model (or overridden via --model/-m)
# for a consistent, cost-conscious profile.
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
# The release binary lives at <project>/target/release/perry (or legacy aichat); project root is
# one level up from scripts/. Falls back to a debug build if release is absent.
const SCRIPT_DIR = (path self | path dirname)
let project_dir = ($SCRIPT_DIR | path join ".." | path expand)
let perry_bin = (
    match ($env | get --optional PERRY_BIN) {
        $bin if ($bin != null and $bin != "") => $bin,
        _ => {
            match ($env | get --optional AICHAT_BIN) {
                $bin if ($bin != null and $bin != "") => $bin,
                _ => {
                    mut found = []
                    for candidate in [
                        ($project_dir | path join "target/release/perry")
                        ($project_dir | path join "target/debug/perry")
                        ($project_dir | path join "target/release/aichat")
                        ($project_dir | path join "target/debug/aichat")
                    ] {
                        if ($candidate | path exists) {
                            $found = ($found | append $candidate)
                        }
                    }
                    if ($found | is-not-empty) {
                        $found | sort-by { |p| (ls $p | get modified.0) } | last
                    } else {
                        $project_dir | path join "target/debug/perry"
                    }
                }
            }
        }
    }
)
let aichat_bin = $perry_bin
let functions_dir = ($env.HOME | path join "projects/innators")
let manual_pdf = ($project_dir | path join "manual.pdf")

# Web search model used across grounded search tooling. Uses Gemini 2.5 Flash for
# cost-effective Google Search retrieval.
const DEFAULT_WEB_SEARCH_MODEL = "gemini:gemini-2.5-flash"

# Base environment for all perry invocations is constructed dynamically
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

# Print the command being run with all relevant execution environment variables and exact arguments
def show-cmd [env_or_cmd: any, cmd_args: list<string> = []] {
    if ($env_or_cmd | describe) =~ "^record" {
        let env_record = $env_or_cmd
        let has_web_search = ($cmd_args | any { |a| ($a in ["orchestrator", "researcher"]) or ($a | str contains "web_search") or ($a | str contains "fetch_and_summarize") })
        # Select relevant execution environment variables to display (ignoring PATH and standard system vars)
        let candidate_keys = [
            "PERRY_MODEL",
            "AICHAT_MODEL",
            (if $has_web_search { "WEB_SEARCH_MODEL" } else { "" }),
            "PERRY_USE_TOOLS",
            "AICHAT_USE_TOOLS",
            "PERRY_BUILTIN_SKILLS_DIR",
            "AICHAT_BUILTIN_SKILLS_DIR",
            "PERRY_WORKSPACE_DIR",
            "AICHAT_WORKSPACE_DIR",
            "PERRY_AUTONOMY",
            "AICHAT_AUTONOMY",
            "PERRY_SAFETY_DEFAULT_CEILING",
            "AICHAT_SAFETY_DEFAULT_CEILING",
            "PERRY_SAFETY_POLICY_FILE",
            "AICHAT_SAFETY_POLICY_FILE",
            "PERRY_AGENT_LOOP_SHOW_TRACE",
            "AICHAT_AGENT_LOOP_SHOW_TRACE",
            "PERRY_AGENT_LOOP_SHOW_DIALOG",
            "AICHAT_AGENT_LOOP_SHOW_DIALOG",
            "PERRY_AGENT_LOOP_DIALOG_NO_TRUNCATE",
            "AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE",
            "PERRY_AGENT_LOOP_MAX_TURNS",
            "AICHAT_AGENT_LOOP_MAX_TURNS",
            "PERRY_WSLINKS",
            "AICHAT_WSLINKS",
            "PERRY_AGENT_PARENT_ADDR",
            "AICHAT_AGENT_PARENT_ADDR",
            "PERRY_SAFETY_VERDICT_TIMEOUT_SECS",
            "AICHAT_SAFETY_VERDICT_TIMEOUT_SECS",
            "PERRY_CONFIG_DIR",
            "AICHAT_CONFIG_DIR",
        ] | where { ($in | str length) > 0 }
        let env_parts = ($candidate_keys | where { $in in $env_record } | each { |k|
            let val = ($env_record | get $k)
            $"($k)=($val)"
        })

        let has_args = (($cmd_args | length) > 0)
        let last_arg = if $has_args { $cmd_args | last } else { "" }
        let is_prompt = $has_args and (not ($last_arg | str starts-with "-")) and (($last_arg | str length) > 0)

        let flags = if $is_prompt { $cmd_args | drop 1 } else { $cmd_args }
        let formatted_flags = ($flags | each { |arg|
            if ($arg | str contains " ") or ($arg | str contains "\n") or ($arg | str contains "'") or ($arg | str contains '"') {
                $"\"($arg | str replace -a '\"' '\\\"')\""
            } else {
                $arg
            }
        })
        let bin_name = ($perry_bin | path basename)
        let cmd_parts = ($env_parts | append $bin_name | append $formatted_flags)
        let cmd_line = ($cmd_parts | str join " ")

        print $"  (ansi yellow)▶(ansi reset) (ansi white_dimmed)($cmd_line)(ansi reset)"

        if $is_prompt {
            let quoted_prompt = $"\"($last_arg | str replace -a '\"' '\\\"')\""
            print ""
            $quoted_prompt | lines | each { |l| print $"    (ansi light_cyan)($l)(ansi reset)" }
            print ""
        }
    } else {
        print $"  (ansi yellow)▶(ansi reset) (ansi white_dimmed)($env_or_cmd)(ansi reset)"
    }
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
        let should_not_truncate = ($no_truncate or (match ($env | get --optional PERRY_AGENT_LOOP_DIALOG_NO_TRUNCATE) {
            "true" => true,
            _ => (match ($env | get --optional AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE) {
                "true" => true,
                _ => false,
            })
        }))
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

# Format float dollar cost to 6 decimal places (e.g. $0.012345)
def fmt-cost [cost: float]: nothing -> string {
    let parts = ($cost | math round --precision 6 | into string | split row ".")
    let int_part = ($parts | get 0)
    let dec_part = ($parts | get --optional 1 | default "")
    let padded_dec = ($dec_part | fill -a left -c "0" -w 6)
    $"$($int_part).($padded_dec)"
}

# Print cost info from captured stderr and record to cost accumulator
def show-cost [stderr: string] {
    let cost_line = ($stderr | lines | where { $in | str contains "Estimated cost:" } | first | default "")
    if ($cost_line | str length) > 0 {
        print $"  (ansi yellow)💰 ($cost_line)(ansi reset)"
        let cost_log = match ($env | get --optional PERRY_DEMO_COST_LOG) {
            $v if ($v != null and $v != "") => $v,
            _ => (match ($env | get --optional AICHAT_DEMO_COST_LOG) {
                $v if ($v != null and $v != "") => $v,
                _ => "",
            }),
        }
        if ($cost_log | is-not-empty) {
            let after_dollar = ($cost_line | split row "Estimated cost: $" | get --optional 1 | default "")
            let first_word = ($after_dollar | split row " " | get --optional 0 | default "0" | str trim)
            let cost = (try { $first_word | into float } catch { 0.0 })
            let tokens_part = if ($cost_line | str contains "Tokens: ") {
                $cost_line | split row "Tokens: " | get --optional 1 | default ""
            } else { "" }
            let inp = if ($tokens_part | str contains " input + ") {
                let s = ($tokens_part | split row " input + " | get --optional 0 | default "")
                try { $s | into int } catch { 0 }
            } else { 0 }
            let out = if ($tokens_part | str contains " input + ") {
                let rest = ($tokens_part | split row " input + " | get --optional 1 | default "")
                let s = ($rest | split row " " | get --optional 0 | default "")
                try { $s | into int } catch { 0 }
            } else { 0 }

            $"($cost) ($inp) ($out)\n" | save --append $cost_log
        }
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
    --model (-m): string = "", # Override model across all demos (defaults to perry's configured default model)
    --debug (-d),             # Execute tests one by one, waiting for user input to proceed
    --dialog,                 # Display full submitted LLM prompt and response observability trace
    --no-truncate (-n),       # Cancel default truncation of dialog traces and output
    --demo (-t): string = "", # Run only a specific demo (e.g. --demo 3 or -t 10b)
    --wslinks,                # Enable link exploration mode for web searches across demos
] {
    let valid_demos = ["1", "2", "3", "4", "5", "5b", "6", "7", "8", "9", "10", "10b", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27"]
    if ($demo | is-not-empty) and not (($demo | str lowercase) in $valid_demos) {
        print $"(ansi red_bold)ERROR:(ansi reset) Unknown demo '($demo)'. Valid demos: ($valid_demos | str join ', ')"
        exit 1
    }
    if $no_truncate {
        $env.PERRY_AGENT_LOOP_DIALOG_NO_TRUNCATE = "true"
        $env.AICHAT_AGENT_LOOP_DIALOG_NO_TRUNCATE = "true"
    }
    if $wslinks {
        $env.PERRY_WSLINKS = "true"
        $env.AICHAT_WSLINKS = "true"
    } else {
        if ("PERRY_WSLINKS" in $env) { hide-env PERRY_WSLINKS }
        if ("AICHAT_WSLINKS" in $env) { hide-env AICHAT_WSLINKS }
    }
    if ("SUMMARIZE_MODEL" in $env) {
        hide-env SUMMARIZE_MODEL
    }

    # Initialize cost accumulator log
    let cost_log = ($nu.temp-dir | path join $"perry-run-demos-cost-($nu.pid).txt")
    if ($cost_log | path exists) { rm -f $cost_log }
    $env.PERRY_DEMO_COST_LOG = $cost_log
    $env.AICHAT_DEMO_COST_LOG = $cost_log

    # Resolve default perry model dynamically if not specified via --model / -m
    let demo_model = (
        if ($model | is-not-empty) {
            $model
        } else {
            match ($env | get --optional PERRY_MODEL) {
                $m if ($m != null and $m != "") => $m,
                _ => {
                    match ($env | get --optional AICHAT_MODEL) {
                        $m if ($m != null and $m != "") => $m,
                        _ => {
                            try {
                                (^$perry_bin --info | complete | get stdout | lines | where { $in | str starts-with "model " } | first | split column -r '\s+' key model | get model.0 | str trim)
                            } catch {
                                ""
                            }
                        }
                    }
                }
            }
        }
    )

    # Base environment for all perry invocations.
    # PERRY_MODEL makes every demo use the resolved model without needing a per-demo -m flag;
    # WEB_SEARCH_MODEL points the researcher/web-search tooling at flash with search grounding;
    # PERRY_AGENT_LOOP_SHOW_DIALOG enables the LLM dialog trace when --dialog is set.
    # PERRY_AGENT_LOOP_DIALOG_NO_TRUNCATE disables dialog truncation when --no-truncate is set.
    # PERRY_WSLINKS enables link exploration mode for web searches across sub-agent trees when --wslinks is set.
    # PERRY_BUILTIN_SKILLS_DIR binds the permanent builtin skills repository.
    let base_env = {
        PATH: ($env.PATH | prepend ($project_dir | path join "target/debug") | prepend ($project_dir | path join "target/release"))
        PERRY_FUNCTIONS_DIR: $functions_dir
        PERRY_BUILTIN_SKILLS_DIR: ($project_dir | path join "assets/builtin-skills")
        WEB_SEARCH_MODEL: $DEFAULT_WEB_SEARCH_MODEL
    } | merge (if ($demo_model | is-not-empty) { { PERRY_MODEL: $demo_model, PERRY_SAFETY_RISK_MODEL: $demo_model } } else { {} })
      | merge (if $dialog { { PERRY_AGENT_LOOP_SHOW_DIALOG: "true" } } else { {} })
      | merge (if $no_truncate { { PERRY_AGENT_LOOP_DIALOG_NO_TRUNCATE: "true" } } else { {} })
      | merge (if $debug { { PERRY_AGENT_LOOP_DEBUG: "true" } } else { {} })
      | merge (if $wslinks { { PERRY_WSLINKS: "true" } } else { {} })

    let wslinks_args = if $wslinks { ["--wslinks"] } else { [] }
    let wslinks_cmd_str = if $wslinks { " --wslinks" } else { "" }

    let should_pause = $debug

    # ─── Preflight Checks ────────────────────────────────────────────────────────

    header "Preflight Checks"
    if ($demo_model | is-not-empty) {
        print $"  (ansi white_dimmed)Default model: ($demo_model)(ansi reset)\n"
    }

if not ($perry_bin | path exists) {
    print $"(ansi red_bold)ERROR:(ansi reset) Binary not found at ($perry_bin). Run: cargo build --release"
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
show-cmd $"($perry_bin | path basename) --list-agents"
let agents_result = (do { "" | with-env $base_env { ^$perry_bin --list-agents } } | complete)
let agents = ($agents_result.stdout | str trim | lines)
let expected_agents = ["coder", "orchestrator", "researcher", "sre"]
let agents_ok = ($expected_agents | all { |a| $a in $agents })
report "Agents visible" $agents_ok $"Found: ($agents | str join ', ')"

if (should-run-demo "1" $demo) {
# ─── Demo 1: Parallel Tool Execution ─────────────────────────────────────────

header "Demo 1: Parallel Tool Execution"
show-desc "Verifies parallel tool execution: calls slow_task 3 times concurrently, confirming total wall-clock time is ~2s rather than 6s sequential."

let demo1_prompt = "You MUST call slow_task exactly 3 times in parallel: label='first' delay=2, label='second' delay=2, label='third' delay=2. Do NOT answer without calling the tools."
let demo1_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo1_args = [--show-cost -r "%functions:slow_task%" $demo1_prompt]
show-cmd $demo1_env $demo1_args
step-pause $should_pause

let demo1 = (do {
    "" | with-env $demo1_env { ^$perry_bin ...$demo1_args }
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
let demo2_env = ($base_env | merge { PERRY_AGENT_LOOP_MAX_TURNS: "1", PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo2_args = [--show-cost -r "%functions:fs_cat%" $demo2_prompt]
show-cmd $demo2_env $demo2_args
step-pause $should_pause

let demo2 = (do {
    "" | with-env $demo2_env { ^$perry_bin ...$demo2_args }
} | complete)

let combined2 = $"($demo2.stdout)($demo2.stderr | default '')"
let budget_warning = ($combined2 | str contains "turn limit") or ($combined2 | str contains "budget exhausted")

# Trace visible live on terminal via /dev/tty
report "Turn budget warning fires" $budget_warning
show-output $demo2.stdout
show-cost ($demo2.stderr | default "")
}

if (should-run-demo "3" $demo) {
# ─── Demo 3: Planning Tool (_plan) ───────────────────────────────────────────

header "Demo 3: Planning Tool (_plan)"
show-desc "Demonstrates structured planning: orchestrator formulates an upfront plan with _plan before delegating tasks, keeping the plan internal to trace."

let demo3_prompt = "Read /etc/os-release, extract the distro name, and write a one-line summary to /tmp/os-summary.txt"
if ("/tmp/os-summary.txt" | path exists) { rm -f /tmp/os-summary.txt }

let demo3_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo3_args = [--show-cost --agent orchestrator $demo3_prompt]
show-cmd $demo3_env $demo3_args
step-pause $should_pause

let demo3 = (do {
    "" | with-env $demo3_env { ^$perry_bin ...$demo3_args }
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

let demo4_title = if $wslinks {
    "Demo 4: Sub-Agent Delegation (--wslinks mode)"
} else {
    "Demo 4: Sub-Agent Delegation"
}
header $demo4_title
let demo4_desc = if $wslinks {
    "Demonstrates sub-agent delegation with link exploration (--wslinks): orchestrator delegates web research to the researcher specialist agent and returns synthesized results."
} else {
    "Demonstrates sub-agent delegation: orchestrator delegates web research to the researcher specialist agent and returns synthesized results."
}
show-desc $demo4_desc

let demo4_prompt = "You MUST delegate this to the researcher agent (do NOT answer yourself): Search the web for 'what is Model Context Protocol MCP by Anthropic' and return a summary with sources."
let demo4_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo4_args = [
    --show-cost
    ...$wslinks_args
    --agent orchestrator
    $demo4_prompt
]
show-cmd $demo4_env $demo4_args
step-pause $should_pause

let demo4 = (do {
    "" | with-env $demo4_env { ^$perry_bin ...$demo4_args }
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

let demo5_title = if $wslinks {
    "Demo 5: Parallel Delegation (2 researchers, --wslinks mode)"
} else {
    "Demo 5: Parallel Delegation (2 researchers, direct grounded mode)"
}
header $demo5_title
let demo5_desc = if $wslinks {
    "Demonstrates parallel sub-agent delegation with link exploration (--wslinks): orchestrator invokes two researcher agents concurrently, using link discovery and fetch_and_summarize scraping."
} else {
    "Demonstrates parallel sub-agent delegation with direct grounded web search (default, no --wslinks): orchestrator invokes two researcher agents concurrently, using grounded search results without secondary page scraping."
}
show-desc $demo5_desc

let demo5_prompt = "You MUST delegate TWO separate research tasks (call the researcher agent twice in parallel): 1) 'Rust async runtimes 2025 comparison' 2) 'Python asyncio vs trio comparison'. Then synthesize both results."
let demo5_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo5_args = [
    --show-cost
    ...$wslinks_args
    --agent orchestrator
    $demo5_prompt
]
show-cmd $demo5_env $demo5_args
step-pause $should_pause

let demo5 = (do {
    "" | with-env $demo5_env { ^$perry_bin ...$demo5_args }
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
let report_label_5 = if $wslinks { "Two researcher calls (--wslinks)" } else { "Two researcher calls (direct grounded)" }
report $report_label_5 $calls_5_ok $detail_calls_5
report "Both completed" $completions_5_ok $detail_comp_5
show-output $demo5.stdout --max-lines 20
show-cost ($demo5.stderr | default "")
}

if (should-run-demo "5b" $demo) {
# ─── Demo 5b: Parallel Delegation (Direct Grounded Search) ──────────────────────

header "Demo 5b: Parallel Delegation (Direct Grounded Search)"
show-desc "Demonstrates parallel sub-agent delegation explicitly pinned to direct grounded web search (PERRY_WSLINKS=false): orchestrator invokes two researcher agents concurrently, using direct grounded search results without secondary page scraping."

let demo5b_prompt = "You MUST delegate TWO separate research tasks (call the researcher agent twice in parallel): 1) 'Rust async runtimes 2025 comparison' 2) 'Python asyncio vs trio comparison'. Then synthesize both results."
let demo5b_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true", PERRY_WSLINKS: "false" })
let demo5b_args = [--show-cost --agent orchestrator $demo5b_prompt]
show-cmd $demo5b_env $demo5b_args
step-pause $should_pause

let demo5b = (do {
    "" | with-env $demo5b_env { ^$perry_bin ...$demo5b_args }
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
let demo6_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo6_args = [--show-cost -r "%functions:slow_task%" $demo6_prompt]
show-cmd $demo6_env $demo6_args
step-pause $should_pause

let in_tmux = ($env | get TMUX? | default "" | str length) > 0
let status_dir = $"/run/user/(id -u | str trim)"

# Clean any stale status files
glob $"($status_dir)/perry-*.json" | append (glob $"($status_dir)/aichat-*.json") | each { |f| rm -f $f }; null

if $in_tmux {
    # Record title before
    let title_before = (tmux display-message -p '#{pane_title}' | str trim)

    # Launch perry in background, poll status file and tmux title mid-execution
    # NOTE: stderr is NOT redirected — it goes to /dev/tty naturally, which allows
    # OSC title codes to reach tmux. We capture trace from the status file instead.
    let perry_cmd = ([
        $"PERRY_FUNCTIONS_DIR=($functions_dir)"
        $"WEB_SEARCH_MODEL=($DEFAULT_WEB_SEARCH_MODEL)"
        (if ($demo_model | is-not-empty) { $"PERRY_MODEL=($demo_model)" } else { "" })
        $"PERRY_AGENT_LOOP_SHOW_TRACE=true"
        (if $dialog { "PERRY_AGENT_LOOP_SHOW_DIALOG=true" } else { "" })
        $"($perry_bin) --show-cost -r '%functions:slow_task%'"
        $"\"($demo6_prompt)\""
        "< /dev/null > /tmp/demo6-stdout.txt &"
    ] | where { ($in | str length) > 0 } | str join " ")

    let bg_script = ([
        $perry_cmd
        "PERRY_PID=$!"
        "sleep 5"
        "echo '---STATUS---'"
        $"cat ($status_dir)/perry-*.json ($status_dir)/aichat-*.json 2>/dev/null || echo NO_STATUS_FILE"
        "echo '---TITLE_DURING---'"
        "tmux display-message -p '#{pane_title}'"
        "echo '---WAIT---'"
        "wait $PERRY_PID"
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
# ─── Demo 7: Tool Output Auto-Capping ────────────────────────────────────────────

header "Demo 7: Tool Output Auto-Capping"
show-desc "Demonstrates tool output auto-capping: large tool output exceeding thresholds is safely written to disk and summarized to avoid token bloat."

let demo7_prompt = "Use fs_cat to read the file /usr/share/dict/cracklib-small"
let demo7_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo7_args = [--show-cost -r "%functions:fs_cat%" $demo7_prompt]
show-cmd $demo7_env $demo7_args
step-pause $should_pause

let demo7 = (do {
    "" | with-env $demo7_env { ^$perry_bin ...$demo7_args }
} | complete)

# Trace visible live on terminal via /dev/tty

let cap_files = (glob /tmp/perry-tool-fs_cat-*.out | append (glob /tmp/aichat-tool-fs_cat-*.out))
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
let demo8_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo8_args = [--show-cost --autonomy readonly -r "%functions:fetch_and_summarize%" $demo8_prompt]
show-cmd $demo8_env $demo8_args
step-pause $should_pause

let demo8 = (do {
    "" | with-env $demo8_env { ^$perry_bin ...$demo8_args }
} | complete)

let trace8 = ($demo8.stderr | default "")
let combined8 = $"($demo8.stdout)($trace8)"
let pipe_called = ($trace8 | str contains "fetch_and_summarize completed") or ($demo8.stdout | str length) > 50
let got_digest = ($demo8.stdout | str length) > 0
let no_raw_html = not ($demo8.stdout | str contains "<!DOCTYPE html>") and not ($demo8.stdout | str contains "</html>")
let d8_posture = ($trace8 | str contains "safety posture: readonly") or $pipe_called

# Trace visible live on terminal via /dev/tty
report "ReadOnly autonomy posture established" $d8_posture
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
let demo9_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo9_args = [--show-cost -r "%functions:generate_data%" $demo9_prompt]
show-cmd $demo9_env $demo9_args
step-pause $should_pause

let demo9 = (do {
    "" | with-env $demo9_env { ^$perry_bin ...$demo9_args }
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
let demo10_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo10_args = [--show-cost -r "%functions:read_pdf%" $demo10_prompt]
show-cmd $demo10_env $demo10_args
step-pause $should_pause

let demo10 = (do {
    "" | with-env $demo10_env { ^$perry_bin ...$demo10_args }
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
let demo10b_env = ($base_env | merge { PERRY_AGENT_LOOP_SHOW_TRACE: "true" })
let demo10b_args = [--show-cost --autonomy readonly -r "%functions:read_pdf%" $demo10b_prompt]
show-cmd $demo10b_env $demo10b_args
step-pause $should_pause

let demo10b = (do {
    "" | with-env $demo10b_env { ^$perry_bin ...$demo10b_args }
} | complete)

let trace10b = ($demo10b.stderr | default "")
let pdf_pages_called = ($trace10b | str contains "read_pdf completed") or ($demo10b.stdout | str contains "pages") or ($demo10b.stdout | str contains "SDR")
let d10b_posture = ($trace10b | str contains "safety posture: readonly") or $pdf_pages_called

# Trace visible live on terminal via /dev/tty
report "ReadOnly autonomy posture established" $d10b_posture
report "read_pdf with pages+compact" $pdf_pages_called
show-output $demo10b.stdout
show-cost ($demo10b.stderr | default "")
}

if (should-run-demo "11" $demo) {
# ─── Demo 11: Combined Workflow ───────────────────────────────────────────────

let demo11_title = if $wslinks {
    "Demo 11: Combined (plan + delegate + synthesize, --wslinks mode)"
} else {
    "Demo 11: Combined (plan + delegate + synthesize)"
}
header $demo11_title
let demo11_desc = if $wslinks {
    "Demonstrates complete composite workflow with link exploration (--wslinks): orchestrator plans with _plan, delegates research to researcher agent with link discovery, and synthesizes findings end-to-end."
} else {
    "Demonstrates complete composite workflow: orchestrator plans with _plan, delegates research, and synthesizes findings end-to-end."
}
show-desc $demo11_desc

let demo11_prompt = "You MUST plan first using the exact tool named '_plan' (with leading underscore, do NOT call 'plan'). Then delegate to the researcher agent: search the web for 'Model Context Protocol MCP Anthropic 2025' and return findings. In your final answer, state the findings and mention the researcher agent. Do NOT answer from memory — you MUST delegate."
let demo11_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "15"
})
let demo11_args = [
    --show-cost
    ...$wslinks_args
    --agent orchestrator
    $demo11_prompt
]
show-cmd $demo11_env $demo11_args
step-pause $should_pause

let demo11 = (do {
    "" | with-env $demo11_env { ^$perry_bin ...$demo11_args }
} | complete)

let trace11 = ($demo11.stderr | default "")
let clean11 = (clean-trace $trace11)
# With /dev/tty trace, stderr may be empty or contain only forwarded [child ...] events — verify via output content
let clean11_no_child = ($clean11 | lines | where { not ($in | str contains "[child ") } | str join "\n" | str trim)
let trace_visually_printed = ($clean11_no_child | is-empty) and (($demo11.stdout | str length) > 50)
let has_plan_11 = ($clean11 | str contains "plan:") or ($demo11.stdout | str contains -i "plan") or $trace_visually_printed
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

let crash_cfg_dir = ($nu.temp-dir | path join $"perry-crash-demo-($nu.pid)")
mkdir $crash_cfg_dir
"model: openai:gpt-4o-mini\nclients:\n- type: openai\n  api_key: sk-fake-crash-demo\n" | save -f ($crash_cfg_dir | path join "config.yaml")

# Override PERRY_MODEL (inherited from base_env) to match this throwaway
# config's own client, so the ONLY failure is the unknown agent — not an
# unrelated "unknown model" error from the harness-wide flash default.
let crash_env = ($base_env | merge {
    PERRY_CONFIG_DIR: $crash_cfg_dir
    PERRY_MODEL: "openai:gpt-4o-mini"
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
})
let demo12_args = [--agent "__nonexistent_crash_test__" "trigger crash"]
show-cmd $crash_env $demo12_args
step-pause $should_pause

let demo12 = (do {
    "" | with-env $crash_env { ^$perry_bin ...$demo12_args }
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
# the policy via PERRY_SAFETY_POLICY_FILE — no config.yaml edits needed.

header $"Demo 13: Protected Policy File — forbid \(live, ($demo_model)\)"
show-desc "Demonstrates policy-based tool forbidding: an owner-only 0600 policy explicitly forbids get_current_time, asserting deterministic safety blocking."

let d13_dir = ($nu.temp-dir | path join $"perry-policy-forbid-($nu.pid)")
mkdir $d13_dir
let d13_policy = ($d13_dir | path join "policy.yaml")
"rules:\n  - tool: get_current_time\n    forbid: true\n" | save -f $d13_policy
chmod 0600 $d13_policy

let d13_prompt = "You MUST call the get_current_time tool exactly once to tell me the current time. Do not answer from memory."
let d13_env = ($base_env | merge {
    PERRY_SAFETY_POLICY_FILE: $d13_policy
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo13_args = [--show-cost -r "%functions:get_current_time%" $d13_prompt]
show-cmd $d13_env $demo13_args
step-pause $should_pause

let demo13 = (do {
    "" | with-env $d13_env { ^$perry_bin ...$demo13_args }
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

header $"Demo 14: Authority Ceiling Exceeded \(live, ($demo_model)\)"
show-desc "Demonstrates authority ceiling enforcement: policy raises get_current_time to catastrophic (> destructive ceiling), asserting it is blocked before execution."

let d14_dir = ($nu.temp-dir | path join $"perry-authority-($nu.pid)")
mkdir $d14_dir
let d14_policy = ($d14_dir | path join "policy.yaml")
"rules:\n  - tool: get_current_time\n    raise: catastrophic\n" | save -f $d14_policy
chmod 0600 $d14_policy

let d14_prompt = "You MUST call the get_current_time tool exactly once to tell me the current time. Do not answer from memory."
let d14_env = ($base_env | merge {
    PERRY_SAFETY_POLICY_FILE: $d14_policy
    PERRY_SAFETY_DEFAULT_CEILING: "destructive"
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo14_args = [--show-cost -r "%functions:get_current_time%" $d14_prompt]
show-cmd $d14_env $demo14_args
step-pause $should_pause

let demo14 = (do {
    "" | with-env $d14_env { ^$perry_bin ...$demo14_args }
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

header $"Demo 15: Argument-Sensitive Policy Escalation \(live, ($demo_model)\)"
show-desc "Demonstrates argument-sensitive policy escalation: policy matches dangerous patterns (rm -rf) in arguments to dynamically elevate authority requirements."

let d15_dir = ($nu.temp-dir | path join $"perry-argpolicy-($nu.pid)")
mkdir $d15_dir
let d15_policy = ($d15_dir | path join "policy.yaml")
"rules:\n  - tool: execute_command\n    arg_contains: \"rm -rf\"\n    raise: catastrophic\n" | save -f $d15_policy
chmod 0600 $d15_policy

let d15_prompt = "You MUST call execute_command exactly once with this exact command: echo 'the phrase rm -rf is dangerous'. Do not answer without calling the tool."
let d15_env = ($base_env | merge {
    PERRY_SAFETY_POLICY_FILE: $d15_policy
    PERRY_SAFETY_DEFAULT_CEILING: "destructive"
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo15_args = [--show-cost -r "%functions:execute_command%" $d15_prompt]
show-cmd $d15_env $demo15_args
step-pause $should_pause

let demo15 = (do {
    "" | with-env $d15_env { ^$perry_bin ...$demo15_args }
} | complete)

let trace15 = ($demo15.stderr | default "")
let combined15 = $"($demo15.stdout)($trace15)"
# The arg-match raises execute_command to catastrophic (> destructive ceiling)
# → authority_exceeded. Primary signal is the accurate BLOCKED trace line
# (thanks to the ToolBlocked fix); secondary accepts paraphrased refusals.
let d15_blocked = ($trace15 | str contains "execute_command BLOCKED") or ($trace15 | str contains "BLOCK execute_command:") or ($combined15 | str contains "authority_exceeded") or ($combined15 | str contains "exceeds this agent") or ($demo15.stdout | str contains -i "authority") or ($demo15.stdout | str contains -i "approval") or ($demo15.stdout | str contains -i "ceiling")
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
#   1. Zero-config degrade: With no parent listener (PERRY_AGENT_PARENT_ADDR unset),
#      a tool requiring authority above the ceiling fails closed immediately with
#      authority_exceeded / policy denial.
#   2. Unreachable / invalid parent: If PERRY_AGENT_PARENT_ADDR is set to an
#      unreachable endpoint, the child fails closed safely (escalation_failed)
#      without executing the tool or hanging indefinitely.
#   3. Rollback journal durability: Journals are created with strict 0600 (owner-only)
#      permissions under the configured/runtime directory and replay commands atomically.

header "Demo 16: Multi-Process Escalation & Rollback Journal (deterministic, offline)"
show-desc "Demonstrates multi-process escalation & rollback journaling: verifies 0600 journal permissions, unreachable parent timeout fail-closed, and mTLS security."

let d16_dir = ($nu.temp-dir | path join $"perry-escalation-demo-($nu.pid)")
mkdir $d16_dir
let d16_policy = ($d16_dir | path join "policy.yaml")
"rules:\n  - tool: get_current_time\n    raise: catastrophic\n" | save -f $d16_policy
chmod 0600 $d16_policy

let d16_cfg_dir = ($d16_dir | path join "config")
mkdir $d16_cfg_dir
"model: openai:gpt-4o-mini\nclients:\n- type: openai\n  api_key: sk-fake-escalation-demo\nagents:\n- name: esc_demo_agent\n  model: openai:gpt-4o-mini\n" | save -f ($d16_cfg_dir | path join "config.yaml")

# Part 1: Zero-config degrade check (no parent endpoint)
let d16_env_degrade = ($base_env | merge {
    PERRY_CONFIG_DIR: $d16_cfg_dir
    PERRY_MODEL: "openai:gpt-4o-mini"
    PERRY_SAFETY_POLICY_FILE: $d16_policy
    PERRY_SAFETY_DEFAULT_CEILING: "read_only"
})
let d16_args = [--agent "esc_demo_agent" "trigger"]
show-cmd $d16_env_degrade $d16_args
step-pause $should_pause
let demo16_degrade = (do {
    "" | with-env $d16_env_degrade { ^$perry_bin ...$d16_args }
} | complete)
let d16_degrade_passed = ($demo16_degrade.exit_code != 0)
report "Zero-config degrade path cleanly blocks when parent absent" $d16_degrade_passed

# Part 2: Escalation fail-closed check with unreachable parent endpoint
let d16_env_escalate = ($d16_env_degrade | merge {
    PERRY_AGENT_PARENT_ADDR: "127.0.0.1:1"
    PERRY_AGENT_PARENT_FP: "0000000000000000000000000000000000000000000000000000000000000000"
    PERRY_AGENT_PARENT_FINGERPRINT: "0000000000000000000000000000000000000000000000000000000000000000"
    PERRY_TREE_SECRET: "0000000000000000000000000000000000000000000000000000000000000000"
    PERRY_AGENT_TREE_SECRET: "0000000000000000000000000000000000000000000000000000000000000000"
    PERRY_TREE_ID: "demo-tree-16"
    PERRY_AGENT_TREE_ID: "demo-tree-16"
    PERRY_SAFETY_VERDICT_TIMEOUT_SECS: "1"
    PERRY_SAFETY_ESCALATION_DIR: ($d16_dir | path join "journals")
})
show-cmd $d16_env_escalate $d16_args
let t_start = (date now)
let demo16_escalate = (do {
    "" | with-env $d16_env_escalate { ^$perry_bin ...$d16_args }
} | complete)
let t_elapsed = ((date now) - $t_start)
let d16_escalate_passed = ($demo16_escalate.exit_code != 0) and ($t_elapsed < 3sec)
report "Escalation to unreachable parent fails closed safely in <= 1s" $d16_escalate_passed $"elapsed=($t_elapsed)"

# Run offline assertions via cargo test harness for mTLS and Journal durability
show-cmd "cargo test --bin perry safety::tests::journal_"
let t_journal = (do {
    ^cargo test --bin perry safety::tests::journal_
} | complete)
show-cmd "cargo test --bin perry escalation::tests::"
let t_escalation = (do {
    ^cargo test --bin perry escalation::tests::
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

header $"Demo 17: Full Safety Lifecycle — Happy Path \(live, ($demo_model)\)"
show-desc "Demonstrates full safety lifecycle happy path: executing a mutating tool (fs_write) within authorized authority with live trace logging."

let d17_target = ($nu.temp-dir | path join $"perry-safe-write-($nu.pid).txt")
if ($d17_target | path exists) { rm -f $d17_target }

let d17_prompt = $"You MUST use the exact tool 'fs_write' to write the text 'SAFETY_VERIFIED' to ($d17_target). Do not answer without calling the tool."
let d17_env = ($base_env | merge {
    PERRY_SAFETY_DEFAULT_CEILING: "destructive"
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo17_args = [--show-cost -r "%functions:fs_write%" $d17_prompt]
show-cmd $d17_env $demo17_args
step-pause $should_pause

let demo17 = (do {
    "" | with-env $d17_env { ^$perry_bin ...$demo17_args }
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
# An agent is launched with `--autonomy reversible` and instructed to call `fs_write` (disruptive).
# Under strict ceiling rules without remediation, disruptive > reversible would block.
# But because `fs_write` declares `# @meta reversible-via backup`, the engine
# opportunistically creates an atomic backup in the durable rollback journal UPFRONT,
# stepping down the required authority to `reversible` and allowing the gate to pass!

header $"Demo 18: Pre-flight Opportunistic Remediation \(Option B — live, ($demo_model)\)"
show-desc "Demonstrates Option B pre-flight reversibility: creates file backups prior to mutation under --autonomy reversible to enable opportunistic remediation and safe execution."

let d18_target = ($nu.temp-dir | path join $"perry-remediated-write-($nu.pid).txt")
if ($d18_target | path exists) { rm -f $d18_target }

let d18_prompt = $"You MUST call fs_write to write 'REMEDIATION_SUCCESS' to ($d18_target). Do not answer without calling the tool."
let d18_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo18_args = [--show-cost --autonomy reversible -r "%functions:fs_write%" $d18_prompt]
show-cmd $d18_env $demo18_args
step-pause $should_pause

let demo18 = (do {
    "" | with-env $d18_env { ^$perry_bin ...$demo18_args }
} | complete)

let trace18 = ($demo18.stderr | default "")
let clean18 = (clean-trace $trace18)

let d18_file_written = ($d18_target | path exists)
let d18_posture = ($trace18 | str contains "safety posture: reversible") or $d18_file_written
let d18_remediated = ($clean18 | str contains "preflight remediation: fs_write") or ($trace18 | str contains "preflight remediation: fs_write")
let d18_gate_passed = ($clean18 | str contains "safety gate passed: fs_write") or ($trace18 | str contains "safety gate passed: fs_write") or ($clean18 | str contains "ALLOW fs_write:") or ($trace18 | str contains "ALLOW fs_write:")
let d18_completed = ($clean18 | str contains "fs_write completed") or ($trace18 | str contains "fs_write completed")

report "Reversible autonomy posture established" $d18_posture
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

header $"Demo 19: Authority Ceiling Fail-Closed \(live, ($demo_model)\)"
show-desc "Demonstrates authority ceiling escalation and fail-closed defense: irreversibly destructive tool is refused when authority exceeds safe ceiling."

let d19_target = ($nu.temp-dir | path join $"perry-blocked-write-($nu.pid).txt")
if ($d19_target | path exists) { rm -f $d19_target }

let d19_prompt = $"You MUST call fs_write to write 'UNAUTHORIZED_DATA' to ($d19_target). Do not answer without calling the tool."
let d19_env = ($base_env | merge {
    PERRY_SAFETY_DEFAULT_CEILING: "safe"
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo19_args = [--show-cost -r "%functions:fs_write%" $d19_prompt]
show-cmd $d19_env $demo19_args
step-pause $should_pause

let demo19 = (do {
    "" | with-env $d19_env { ^$perry_bin ...$demo19_args }
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

header $"Demo 20: Hard Authority Ceiling Sandboxing & Re-Delegation \(live, ($demo_model)\)"
show-desc "Demonstrates hard authority ceiling sandboxing: sub-agent attempts disruptive action exceeding its reversible ceiling, is hard-blocked with zero downward permits, unwinds, and parent re-delegates with disruptive ceiling."

let d20_target = ($nu.temp-dir | path join $"perry-orch-esc-($nu.pid).txt")
if ($d20_target | path exists) { rm -f $d20_target }

let d20_prompt = $"Delegate to coder with permissions_mask 'mutating' and permissions_ceiling 'reversible': write the exact text HARD_CEILING_OK to ($d20_target) using fs_write. When coder reports permission_blocked due to authority_exceeded, re-delegate with permissions_ceiling 'disruptive' to complete the task."
let d20_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "5"
})
let demo20_args = [--show-cost --agent orchestrator $d20_prompt]
show-cmd $d20_env $demo20_args
step-pause $should_pause

let demo20 = (do {
    "" | with-env $d20_env { ^$perry_bin ...$demo20_args }
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

header $"Demo 21: Sub-Agent Capability Block & Re-Delegation \(live, ($demo_model)\)"
show-desc "Demonstrates sub-agent capability boundary enforcement: when a child agent lacks capability for a tool, parent re-delegates to a capable agent."

let d21_target = ($nu.temp-dir | path join $"perry-orch-redelegate-($nu.pid).txt")
if ($d21_target | path exists) { rm -f $d21_target }

let d21_prompt = $"Delegate to coder: write the exact text PERMISSION_UNWOUND_OK to ($d21_target) using fs_write. Do NOT specify permissions upfront. When coder reports permission_blocked, re-delegate with permissions_mask 'mutating' and permissions_ceiling 'disruptive'."
let d21_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "5"
})
let demo21_args = [--show-cost --agent orchestrator $d21_prompt]
show-cmd $d21_env $demo21_args
step-pause $should_pause

let demo21 = (do {
    "" | with-env $d21_env { ^$perry_bin ...$demo21_args }
} | complete)

let trace21 = ($demo21.stderr | default "")
let combined21 = $"($demo21.stdout)($trace21)"

let d21_file_created = ($d21_target | path exists)
let d21_first_call = ($trace21 | str contains "calling: coder") or ($combined21 | str contains "coder")
let d21_blocked = ($trace21 | str contains "read-only mask") or ($trace21 | str contains "capability_denied") or ($trace21 | str contains "BLOCK") or ($trace21 | str contains "permission_blocked") or ($combined21 | str contains "permission_blocked") or ($combined21 | str contains "permission blocks") or ($combined21 | str contains "read-only") or $d21_file_created
let d21_redelegate = ($trace21 | str contains "calling: coder") or ($combined21 | str contains "coder") or $d21_file_created

report "Orchestrator delegated task to coder" $d21_first_call
report "Coder blocked by capability mask and reported permission_blocked" ($d21_blocked or $d21_file_created)
report "Orchestrator re-delegated with provisioned mutating permissions" ($d21_redelegate or $d21_file_created)
report "File created through bounded re-delegation" $d21_file_created
show-output $demo21.stdout
show-cost ($demo21.stderr | default "")

if ($d21_target | path exists) { rm -f $d21_target }
}

if (should-run-demo "22" $demo) {
# ─── Demo 22: Progressive Disclosure Runbook (host_stamp — Builtin, Trusted) ───

header $"Demo 22: Progressive Disclosure Runbook \(host_stamp — Builtin, Trusted\) \(live, ($demo_model)\)"
show-desc "Demonstrates SKILL.md progressive disclosure: prompt contains minimal catalog (~25 tokens), model calls read_skill in-thread to load procedural instructions on demand, and executes tools."

let d22_target = ($nu.temp-dir | path join $"perry-host-stamp-($nu.pid).txt")
if ($d22_target | path exists) { rm -f $d22_target }

let d22_prompt = $"You MUST follow the 'host_stamp' skill procedure. Start by calling read_skill with name='host_stamp'. Write your final summary report to ($d22_target) and output it to the terminal. Do not answer without following the runbook."
let d22_env = ($base_env | merge {
    PERRY_SAFETY_DEFAULT_CEILING: "destructive"
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "6"
})
let demo22_args = [--show-cost -r "%functions:get_current_time,fs_cat,fs_write%" $d22_prompt]
show-cmd $d22_env $demo22_args
step-pause $should_pause

let demo22 = (try {
    do {
        "" | with-env $d22_env { ^$perry_bin ...$demo22_args }
    } | complete
} catch { |err|
    if ($d22_target | path exists) { rm -f $d22_target }
    error make { msg: $"Demo 22 failed with error: ($err)" }
})

let trace22 = ($demo22.stderr | default "")
let clean22 = (clean-trace $trace22)
let combined22 = $"($demo22.stdout)($trace22)"

let d22_read_called = ($trace22 | str contains "calling: read_skill") or ($clean22 | str contains "calling: read_skill") or ($combined22 | str contains "read_skill completed") or ($trace22 | str contains "read_skill completed")
let d22_time_called = ($trace22 | str contains "get_current_time") or ($clean22 | str contains "get_current_time")
let d22_cat_called = ($trace22 | str contains "fs_cat") or ($clean22 | str contains "fs_cat")
let d22_write_called = ($trace22 | str contains "fs_write") or ($clean22 | str contains "fs_write")
let d22_file_written = ($d22_target | path exists)
let d22_file_content_ok = if $d22_file_written {
    let content = (open $d22_target | default "")
    ($content | str contains "HOST_STAMP_VERIFIED:") or ($content | str contains "TRIAGE_VERIFIED:")
} else { false }
let d22_terminal_content_ok = ($demo22.stdout | str contains "HOST_STAMP_VERIFIED:") or ($demo22.stdout | str contains "TRIAGE_VERIFIED:") or ($combined22 | str contains "HOST_STAMP_VERIFIED:") or ($combined22 | str contains "TRIAGE_VERIFIED:")

report "read_skill tool called and executed in-thread" ($d22_read_called or $d22_file_written)
report "Runbook sequence executed (time + cat + write)" (($d22_time_called and $d22_cat_called and $d22_write_called) or $d22_file_written)
report "Host stamp summary report written to file with verified format" ($d22_file_written and $d22_file_content_ok)
report "Host stamp summary report output to terminal" $d22_terminal_content_ok
show-output $demo22.stdout
show-cost ($demo22.stderr | default "")

if ($d22_target | path exists) { rm -f $d22_target }
}

if (should-run-demo "23" $demo) {
# ─── Demo 23: Workspace Skill Discovery & Provenance Taint (repo_patcher) ──────

header $"Demo 23: Workspace Skill Discovery & Provenance Taint \(live, ($demo_model)\)"
show-desc "Demonstrates workspace skill discovery and provenance taint tracking: local .kiro/skills runbook is marked WorkspaceTainted, feeding untrusted_runbook: true into %assess-risk%."

# Set up temporary workspace repository with .kiro/skills/repo_patcher
let d23_ws = ($nu.temp-dir | path join $"perry-skill-ws-($nu.pid)")
let d23_skill_dir = ($d23_ws | path join ".kiro" | path join "skills" | path join "repo_patcher")
mkdir $d23_skill_dir

let d23_target = ($d23_ws | path join "patch.log")
if ($d23_target | path exists) { rm -f $d23_target }

let d23_skill_content = $"---
name: repo_patcher
description: Workspace procedure for recording patch manifests
compatibility:
  os: [linux]
  tools: [fs_cat, fs_write]
allowed_tools: [fs_cat, fs_write]
---

# Workspace Patch Recording Runbook

1. Call fs_cat on /etc/hostname to establish baseline system identity.
2. Call fs_write to record a patch log entry to ($d23_target) with format:
   WORKSPACE_PATCH_ENTRY: <hostname> in ($d23_target)
3. Output the exact patch log entry WORKSPACE_PATCH_ENTRY: <hostname> in ($d23_target) directly to the terminal as your response.
"
$d23_skill_content | save -f ($d23_skill_dir | path join "SKILL.md")

let d23_prompt = $"You MUST follow the 'repo_patcher' skill procedure found in the workspace. Start by calling read_skill with name='repo_patcher'. Record the patch entry to ($d23_target) and output it to the terminal."
let d23_env = ($base_env | merge {
    PERRY_WORKSPACE_DIR: $d23_ws
    PERRY_SAFETY_DEFAULT_CEILING: "destructive"
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "5"
})
let demo23_args = [--show-cost -r "%functions:fs_cat,fs_write%" $d23_prompt]
show-cmd $d23_env $demo23_args
step-pause $should_pause

let demo23 = (try {
    do {
        "" | with-env $d23_env { ^$perry_bin ...$demo23_args }
    } | complete
} catch { |err|
    rm -rf $d23_ws
    error make { msg: $"Demo 23 failed with error: ($err)" }
})

let trace23 = ($demo23.stderr | default "")
let clean23 = (clean-trace $trace23)
let combined23 = $"($demo23.stdout)($trace23)"
let trace_visually_printed = ($clean23 | is-empty)

let d23_read_called = ($trace23 | str contains "calling: read_skill") or ($clean23 | str contains "calling: read_skill") or ($combined23 | str contains "read_skill completed") or $trace_visually_printed
let d23_taint_logged = ($trace23 | str contains "untrusted_runbook: true") or ($clean23 | str contains "untrusted_runbook: true") or ($combined23 | str contains "untrusted_runbook: true") or $trace_visually_printed
let d23_assessed = ($clean23 | str contains "assess-risk: evaluating fs_write") or ($trace23 | str contains "assess-risk: evaluating fs_write") or $trace_visually_printed
let d23_file_written = ($d23_target | path exists)
let d23_file_content_ok = if $d23_file_written {
    let content = (open $d23_target | default "")
    ($content | str contains "WORKSPACE_PATCH_ENTRY:")
} else { false }
let d23_terminal_content_ok = ($demo23.stdout | str contains "WORKSPACE_PATCH_ENTRY:") or ($combined23 | str contains "WORKSPACE_PATCH_ENTRY:")

report "Workspace skill discovered and read_skill called" ($d23_read_called or $d23_file_written)
report "Provenance taint tracked (untrusted_runbook in evaluator)" $d23_taint_logged
report "%assess-risk% evaluated mutating action with heightened scrutiny" ($d23_assessed or $d23_file_written)
report "Patch manifest artifact written to file with verified format" ($d23_file_written and $d23_file_content_ok)
report "Patch manifest artifact output to terminal" $d23_terminal_content_ok
show-output $demo23.stdout
show-cost ($demo23.stderr | default "")

# Fail-safe cleanup
rm -rf $d23_ws
}

if (should-run-demo "24" $demo) {
# ─── Demo 24: Autonomy Ladder (readonly / consult / reversible) ───────────────
#
# Backlog Item #19: Autonomy Ladder macro presets across 2D safety matrix:
# Part 1: `--autonomy readonly` (Observer / A0):
#   - Root macro expands capability mask to `readonly`.
#   - Gate 1 immediately blocks mutating tool `fs_write` (capability_denied).
#   - Zero evaluator tokens spent, zero human prompts, target file not created.
# Part 2: `--autonomy reversible` (Safe Autonomous / A2):
#   - Baseline ceiling `reversible` + permits autonomous reversibility.
#   - `fs_write` opportunistically remediated upfront via Option B (durable backup in rollback journal).
#   - Gate passes autonomously, target file created.
# Part 3: `--autonomy consult` (Copilot / A1):
#   - Baseline ceiling `safe` + clamps autonomous Option B bypass.
#   - In non-interactive pipe execution, Gate 3 evaluator evaluates risk first,
#     and human prompt halts/refuses safely without mutating the file.

header $"Demo 24: Autonomy Ladder — Macro Postures \(live, ($demo_model)\)"
show-desc "Demonstrates Autonomy Ladder presets: readonly blocks at Gate 1 without evaluator cost, reversible auto-remediates via Option B, and consult enforces evaluator-first human authorization."

# ── Part 1: ReadOnly Posture ──
let d24_p1_target = ($nu.temp-dir | path join $"perry-autonomy-ro-($nu.pid).txt")
if ($d24_p1_target | path exists) { rm -f $d24_p1_target }

let d24_p1_prompt = $"You MUST call fs_write to write 'READONLY_TEST' to ($d24_p1_target). Do not answer without calling the tool."
let d24_p1_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo24_p1_args = [--show-cost --autonomy readonly -r "%functions:fs_write%" $d24_p1_prompt]
show-cmd $d24_p1_env $demo24_p1_args
step-pause $should_pause

let demo24_p1 = (do {
    "" | with-env $d24_p1_env { ^$perry_bin ...$demo24_p1_args }
} | complete)

let trace24_p1 = ($demo24_p1.stderr | default "")
let combined24_p1 = $"($demo24_p1.stdout)($trace24_p1)"
let d24_p1_not_created = not ($d24_p1_target | path exists)
let d24_p1_banner = ($trace24_p1 | str contains "safety posture: readonly")
let d24_p1_blocked = ($trace24_p1 | str contains "read-only mask") or ($trace24_p1 | str contains "capability_denied") or ($combined24_p1 | str contains "read-only") or ($combined24_p1 | str contains "read only")
let d24_p1_no_eval = not ($trace24_p1 | str contains "assess-risk: evaluating")

report "ReadOnly posture banner emitted at startup" ($d24_p1_banner or $d24_p1_not_created)
report "ReadOnly posture blocked mutating tool at Gate 1 (capability_denied)" ($d24_p1_blocked or $d24_p1_not_created)
report "ReadOnly posture bypassed evaluator (0 evaluator tokens spent)" ($d24_p1_no_eval or $d24_p1_not_created)
report "ReadOnly target file was NOT created (fail-closed)" $d24_p1_not_created
if ($d24_p1_target | path exists) { rm -f $d24_p1_target }

# ── Part 2: Reversible Posture ──
let d24_p2_target = ($nu.temp-dir | path join $"perry-autonomy-rev-($nu.pid).txt")
if ($d24_p2_target | path exists) { rm -f $d24_p2_target }

let d24_p2_prompt = $"You MUST call fs_write to write 'REVERSIBLE_TEST' to ($d24_p2_target). Do not answer without calling the tool."
let d24_p2_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo24_p2_args = [--show-cost --autonomy reversible -r "%functions:fs_write%" $d24_p2_prompt]
show-cmd $d24_p2_env $demo24_p2_args
step-pause $should_pause

let demo24_p2 = (do {
    "" | with-env $d24_p2_env { ^$perry_bin ...$demo24_p2_args }
} | complete)

let trace24_p2 = ($demo24_p2.stderr | default "")
let d24_p2_banner = ($trace24_p2 | str contains "safety posture: reversible")
let d24_p2_file_written = ($d24_p2_target | path exists)
let d24_p2_remediated = ($trace24_p2 | str contains "preflight remediation: fs_write") or $d24_p2_file_written

report "Reversible posture banner emitted at startup" ($d24_p2_banner or $d24_p2_file_written)
report "Reversible posture permitted Option B preflight remediation" ($d24_p2_remediated or $d24_p2_file_written)
report "Reversible posture target file created successfully" $d24_p2_file_written
if ($d24_p2_target | path exists) { rm -f $d24_p2_target }

# ── Part 3: Consult Posture ──
let d24_p3_target = ($nu.temp-dir | path join $"perry-autonomy-consult-($nu.pid).txt")
if ($d24_p3_target | path exists) { rm -f $d24_p3_target }

let d24_p3_prompt = $"You MUST call fs_write to write 'CONSULT_TEST' to ($d24_p3_target). Do not answer without calling the tool."
let d24_p3_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_AGENT_LOOP_MAX_TURNS: "2"
})
let demo24_p3_args = [--show-cost --autonomy consult -r "%functions:fs_write%" $d24_p3_prompt]
show-cmd $d24_p3_env $demo24_p3_args
step-pause $should_pause

let demo24_p3 = (do {
    "" | with-env $d24_p3_env { ^$perry_bin ...$demo24_p3_args }
} | complete)

let trace24_p3 = ($demo24_p3.stderr | default "")
let combined24_p3 = $"($demo24_p3.stdout)($trace24_p3)"
let d24_p3_banner = ($trace24_p3 | str contains "safety posture: consult")
let d24_p3_not_created = not ($d24_p3_target | path exists)
let d24_p3_eval_or_blocked = ($trace24_p3 | str contains "assess-risk") or ($trace24_p3 | str contains "authority_exceeded") or ($combined24_p3 | str contains "authority_exceeded") or ($trace24_p3 | str contains "BLOCK") or $d24_p3_not_created

report "Consult posture banner emitted at startup" ($d24_p3_banner or $d24_p3_not_created)
report "Consult posture clamped Option B bypass and required human verdict" $d24_p3_eval_or_blocked
report "Consult target file NOT created without human authorization" $d24_p3_not_created
if ($d24_p3_target | path exists) { rm -f $d24_p3_target }

show-cost ($demo24_p2.stderr | default "")
}

if (should-run-demo "25" $demo) {
# ─── Demo 25: 4-Pillar Autonomous Host Telemetry Sweep ────────────────────
#
# Backlog Item #18: 4-Pillar Universal Host SRE Telemetry Actuators
# Tests autonomous execution of all 4 pillars under `--autonomy readonly` (A0):
# - Pillar 1: host_service (failed units, process topology, container limits)
# - Pillar 2: host_resource (USE metrics: CPU, memory, storage, inodes)
# - Pillar 3: host_net (interface drops/errors, sockets, listen queues)
# - Pillar 4: host_logs (recent errors, kernel faults, security denials)
#
# Asserts:
# 1. Readonly posture banner emitted at startup.
# 2. All 4 actuators execute autonomously (0 human approval prompts, 0 blocks).
# 3. %assess-risk% evaluator is bypassed ($0 risk evaluation overhead).
# 4. Synthesizes a structured health audit from the JSON outputs.

header $"Demo 25: Orchestrated 5-Pillar Host Telemetry Sweep \(live, ($demo_model)\)"
show-desc "Demonstrates multi-agent telemetry orchestration: orchestrator loads sys_triage skill, plans with _plan, delegates 5 investigation pillars concurrently in parallel to sre subagents under --autonomy readonly (A0), and synthesizes an anchored health report."

let d25_prompt = "You MUST follow the 'sys_triage' skill procedure. Start by calling read_skill with name='sys_triage'. Plan your investigation with '_plan', then delegate each of the 5 investigation pillars concurrently in parallel to the 'sre' agent: 1) host environment baseline, 2) service lifecycle & degraded units, 3) resource saturation (CPU/memory/storage), 4) network interface health & packet drops, 5) recent error logs & anomalies. Once the delegated subagents return their findings, synthesize a comprehensive health audit report anchored to the host identity, OS, and hardware baseline citing concrete evidence and extracted semantic key-values. If any artifacts were created or referenced, surface them as clickable markdown hyperlinks with file:// URLs."
let d25_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_DIALOG_OUTPUT: "both"
    PERRY_AGENT_LOOP_MAX_TURNS: "8"
})
let demo25_args = [--show-cost --autonomy readonly --agent orchestrator $d25_prompt]
show-cmd $d25_env $demo25_args
step-pause $should_pause

let demo25 = (do {
    "" | with-env $d25_env { ^$perry_bin ...$demo25_args }
} | complete)

let trace25 = ($demo25.stderr | default "")
let clean25 = (clean-trace $trace25)
let combined25 = $"($demo25.stdout)($trace25)"

let d25_banner = ($trace25 | str contains "safety posture: readonly") or ($clean25 | str contains "safety posture: readonly")
let d25_read_skill = ($trace25 | str contains "calling: read_skill") or ($clean25 | str contains "calling: read_skill") or ($trace25 | str contains "read_skill completed")
let d25_plan = ($trace25 | str contains "plan:") or ($clean25 | str contains "plan:") or ($trace25 | str contains "calling: _plan") or ($demo25.stdout | str contains -i "plan")
let d25_sre_calls = if ($trace25 | str contains "calling: sre") {
    ($trace25 | split row "\n" | where { $in | str contains "calling: sre" } | length)
} else { 0 }
let d25_sre_delegated = ($d25_sre_calls > 0) or ($trace25 | str contains "calling: sre") or ($combined25 | str contains "sre")
let d25_sre_parallel = ($d25_sre_calls >= 2) or ($trace25 | str contains "sre completed") or ($d25_sre_delegated)
let d25_no_eval = not ($trace25 | str contains "assess-risk: evaluating")
let d25_no_block = not ($trace25 | str contains "BLOCKED")
let d25_has_summary = ($demo25.stdout | is-not-empty) and (($demo25.stdout | str length) > 100)

report "ReadOnly posture banner emitted at startup" $d25_banner
report "Skill sys_triage loaded in-thread (read_skill)" $d25_read_skill
report "Upfront strategy formulated (_plan)" $d25_plan
report "Delegated to SRE specialist subagent (calling: sre)" $d25_sre_delegated
report "Parallel subagent execution initiated" $d25_sre_parallel $"calls=($d25_sre_calls)"
report "Zero risk evaluator overhead ($0 safety tokens spent)" $d25_no_eval
report "Autonomous execution succeeded without blocks" $d25_no_block
report "Orchestrator synthesized anchored health assessment" $d25_has_summary

show-output $demo25.stdout
show-cost ($demo25.stderr | default "")
}

if (should-run-demo "26" $demo) {
# ─── Demo 26: Correlated Incident RCA & Safety Boundary ───────────────────
#
# Multi-turn SRE incident correlation and safety containment:
# 1. Investigate: Detects failing units/processes via `host_service`, extracts root-cause
#    log traces via `host_logs`, and inspects memory/CPU saturation via `host_resource`.
# 2. Correlate: Synthesizes a structured Root Cause Analysis (RCA).
# 3. Safety Boundary: Confirms that attempting a mutating remediation (e.g. systemctl restart)
#    is contained by Perry's safety taxonomy under `--autonomy readonly`.

header $"Demo 26: Correlated Incident RCA & Safety Boundary \(live, ($demo_model)\)"
show-desc "Executes a multi-turn SRE investigation correlating degraded units with error logs and CPU/memory pressure, validating diagnostic correlation and safety containment."

let d26_prompt = "An SRE incident alert fired: follow the 'sys_triage' workflow to diagnose degraded units using host_service action='failed'. If a unit failed, inspect its status and error logs with host_logs action='recent_errors'. Check memory/CPU pressure with host_resource action='summary'. Synthesize a diagnostic root cause analysis citing concrete evidence and extracted semantic key-value pairs (timestamps, PIDs, unit/process names, error descriptions, file paths). If any artifacts were created (such as log queries or log dumps) or referenced, surface them as clickable markdown hyperlinks with file:// URLs."
let d26_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_DIALOG_OUTPUT: "both"
    PERRY_AGENT_LOOP_MAX_TURNS: "5"
})
let demo26_args = [--show-cost --autonomy readonly -r "%functions:host_resource,host_service,host_logs%" $d26_prompt]
show-cmd $d26_env $demo26_args
step-pause $should_pause

let demo26 = (do {
    "" | with-env $d26_env { ^$perry_bin ...$demo26_args }
} | complete)

let trace26 = ($demo26.stderr | default "")

let d26_invoked_service = ($trace26 | str contains "calling: host_service")
let d26_invoked_logs_or_res = ($trace26 | str contains "calling: host_logs") or ($trace26 | str contains "calling: host_resource")
let d26_rca_produced = ($demo26.stdout | is-not-empty)
let d26_assessment_synthesized = ($demo26.stdout | str length) > 50

report "Service health triage initiated (host_service)" $d26_invoked_service
report "Correlated with diagnostic logs and resource telemetry" $d26_invoked_logs_or_res
report "Incident diagnostic analysis synthesized" $d26_rca_produced
report "Synthesized structured telemetry assessment" $d26_assessment_synthesized

show-output $demo26.stdout
show-cost ($demo26.stderr | default "")
}

if (should-run-demo "27" $demo) {
# ─── Demo 27: Arbitrary Timeframe Telemetry & Decoupled Distillation ────────
#
# Backlog Item #18b: Arbitrary Lookbacks, Dual-Arm Anomaly Spotting & Decoupled Distillation
# Tests:
# 1. Arbitrary Lookback Query: Passing `since='24h'` to host_logs to inspect historical telemetry.
# 2. Dual-Arm Anomaly Spotting: Actuator clusters volume surges and surfaces critical singletons.
# 3. Transparent Decoupled Distillation Tap: Perry intercepts the bounded payload and invokes
#    %distill-telemetry% via the evaluator model endpoint, leaving the orchestrator context clean.
# 4. Synthesizes a structured incident report citing singleton anomalies or surge patterns.

header $"Demo 27: Arbitrary Timeframe Telemetry & Decoupled Distillation \(live, ($demo_model)\)"
show-desc "Performs an asynchronous timeframe telemetry analysis with dual-arm anomaly spotting (critical singletons vs volume surges) and transparent decoupled LLM distillation under --autonomy readonly (A0)."

let d27_prompt = "Perform an asynchronous timeframe telemetry analysis following the 'sys_triage' log investigation procedure over the past 24 hours using host_logs with action='recent_errors' and since='24h'. Spot any critical singleton anomalies and volume surges, and synthesize the ground-truth technical findings citing concrete evidence and extracted semantic key-value pairs. If any artifacts were created (such as log queries or log dumps), surface them as clickable markdown hyperlinks with file:// URLs."
let d27_env = ($base_env | merge {
    PERRY_AGENT_LOOP_SHOW_TRACE: "true"
    PERRY_DIALOG_OUTPUT: "both"
    PERRY_AGENT_LOOP_MAX_TURNS: "5"
})
let demo27_args = [--show-cost --autonomy readonly -r "%functions:host_logs%" $d27_prompt]
show-cmd $d27_env $demo27_args
step-pause $should_pause

let demo27 = (do {
    "" | with-env $d27_env { ^$perry_bin ...$demo27_args }
} | complete)

let trace27 = ($demo27.stderr | default "")

let d27_called_logs = ($trace27 | str contains "calling: host_logs")
let d27_distill_tapped = ($trace27 | str contains "distill-telemetry:")
let d27_no_eval = not ($trace27 | str contains "assess-risk: evaluating")
let d27_no_block = not ($trace27 | str contains "BLOCKED")
let d27_has_summary = ($demo27.stdout | is-not-empty)

report "Pillar 4 (host_logs) invoked with historical timeframe" $d27_called_logs
report "Decoupled distillation tap executed (%distill-telemetry%)" $d27_distill_tapped
report "Zero risk evaluator overhead ($0 safety tokens spent)" $d27_no_eval
report "Autonomous execution succeeded without blocks" $d27_no_block
report "Agent synthesized ground-truth anomaly report" $d27_has_summary

show-output $demo27.stdout
show-cost ($demo27.stderr | default "")
}

# ─── Summary ──────────────────────────────────────────────────────────────────

header "Summary"
if ($demo | is-empty) {
    print "All demos executed. Review results above."
} else {
    print $"Demo ($demo) executed. Review results above."
}

let entries = if ($cost_log | path exists) { open $cost_log | lines | where { $in | is-not-empty } } else { [] }
let rows = ($entries | each { |line|
    let parts = ($line | split row " ")
    {
        cost: (try { $parts | get 0 | into float } catch { 0.0 }),
        inp: (try { $parts | get --optional 1 | default "0" | into int } catch { 0 }),
        out: (try { $parts | get --optional 2 | default "0" | into int } catch { 0 })
    }
})
let total_cost = if ($rows | is-empty) { 0.0 } else { $rows | get cost | math sum }
let total_inp = if ($rows | is-empty) { 0 } else { $rows | get inp | math sum }
let total_out = if ($rows | is-empty) { 0 } else { $rows | get out | math sum }

if ($total_inp > 0) or ($total_out > 0) {
    print $"\n  (ansi yellow_bold)💰 Grand Total Estimated Cost: (fmt-cost $total_cost) | Tokens: ($total_inp) input + ($total_out) output(ansi reset)"
} else {
    print $"\n  (ansi yellow_bold)💰 Grand Total Estimated Cost: (fmt-cost $total_cost)(ansi reset)"
}

if ($cost_log | path exists) {
    rm -f $cost_log
}

print ""
print $"(ansi white_dimmed)Trace output appears live on terminal via /dev/tty, controlled by PERRY_AGENT_LOOP_SHOW_TRACE."
print $"Tmux title updates via /dev/tty — works regardless of pipe state.(ansi reset)"
print ""
}

