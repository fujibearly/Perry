# Lessons Learned & Agent Insights

This document captures architectural lessons, debugging insights, and operational principles discovered during the development and maintenance of `aichat`. Every insight is **postmarked** (timestamped) to provide chronological context and provenance for future agents and engineers.

---

### [2026-09-16T20:15:00-04:00] Governance Pipeline Sequencing: Avoid Premature `ALLOW` Emissions
- **Category:** Safety & Governance / Agent Loop
- **Problem:**
  In a multi-phase governance architecture (e.g., Phase 1: deterministic static authority ceiling check $\to$ Phase 2: dynamic LLM `%assess-risk%` evaluation), `authority_denied_result` previously emitted `SafetyGatePassed` (`ALLOW <tool>: risk <tier> <= ceiling <tier>`) before `%assess-risk%` had even been invoked.
- **Consequence:**
  To operators and log parsers, the tool was prematurely declared "allowed" before dynamic scrutiny took place. If the dynamic evaluator subsequently elevated the risk or blocked the action, the trace displayed contradictory states (`ALLOW` followed by `BLOCK` or risk assessment).
- **Resolution:**
  When an action is slated for dynamic risk evaluation (`will_consult_risk_evaluator`), the static preflight gate verifies ceiling compatibility silently. The `ALLOW` event is deferred until the dynamic evaluator has completed and verified that the post-evaluation clamped risk tier remains within the authority ceiling.
- **Actionable Rule for Agents:**
  *Never emit an authorization event (`ALLOW`) at an intermediate gate if downstream gates retain veto or escalation authority. An action is only ALLOWED when all gates have completed.*

---

### [2026-09-16T20:15:00-04:00] Pseudo-Tools Require Explicit Classification in Safety Envelopes
- **Category:** Tool Safety & Classification
- **Problem:**
  Dynamic built-in pseudo-tools (such as `read_skill` in Backlog #17 or `_plan` in Backlog #15) execute in-thread rather than as external binaries in `tools/*.sh`. When `read_skill` was introduced, it was omitted from the safety gate lookup helpers (`tool_safety_class`, `tool_tier_and_reversibility`, `find_tool_declaration`).
- **Consequence:**
  The engine's strict fail-closed policy kicked in: unclassified tools default to `StaticTier::Unclassified` $\to$ `BlastRadius::Catastrophic` $\to$ `RequiredAuthority::Human`. Autonomous agents running under standard ceilings (`destructive`, `reversible`, or `readonly`) were instantly hard-blocked with `authority_exceeded` whenever attempting to read a skill.
- **Resolution:**
  Explicitly classified `read_skill` as `SafetyClass::Readonly`, `BlastRadius::Safe`, and `intrinsic_reversible: true`.
- **Actionable Rule for Agents:**
  *Any newly introduced pseudo-tool or engine-internal function declaration MUST be registered across all governance envelopes (`tool_safety_class`, `tool_tier_and_reversibility`, `find_tool_declaration`). Verify new tools against the static gate before wiring them into prompts.*

---

### [2026-09-16T20:15:00-04:00] Capability Disclosure Must Hook Both Non-Agent Roles and Agents
- **Category:** Configuration & Prompt Engineering
- **Problem:**
  Progressive disclosure mechanisms (`### Available Skills` catalog injection) were originally wired only into the agent initialization path (`agent.prompt`). When running non-agent functional roles (e.g., `-r "%functions:get_current_time,fs_cat%"`), the role prompt had no skills catalog injected, and `read_skill` was not in the tool whitelist.
- **Consequence:**
  One-shot CLI runs and functional roles could not leverage on-demand procedural runbooks without configuring a full multi-turn agent profile.
- **Resolution:**
  1. Added `role.append_prompt(...)` to dynamically augment non-agent roles in `src/config/role.rs`.
  2. In `src/config/mod.rs` (`extract_role`), if `self.agent.is_none()` and eligible skills exist, inject `format_prompt_catalogue(&eligible)`.
  3. In `select_functions()`, implicitly inject `read_skill` whenever eligible skills are present, ensuring tool availability even under scoped roles.
- **Actionable Rule for Agents:**
  *When augmenting system prompts with metadata catalogs or dynamic capabilities, ensure both full agent profiles and non-agent CLI roles (`Role`) are hooked.*

---

### [2026-09-16T20:15:00-04:00] Schema Token Budgeting: Scoped Functional Roles
- **Category:** Performance & Token Efficiency
- **Problem:**
  Specifying `-r "%functions%"` injected full JSON Schema definitions for all 31 tools into every LLM request. Across multi-turn agent conversations, this consumed ~6,000 input tokens per turn (~30,000+ tokens across a 5-turn dialog) purely for schema overhead.
- **Consequence:**
  High latency, unnecessary API cost, and increased risk of tool hallucination due to schema clutter.
- **Resolution:**
  Leveraged scoped functional role syntax: `-r "%functions:tool1,tool2%"`. Updated all 14 functional demos in `scripts/run-demos.nu`, reducing schema token consumption by ~95% (from ~6,000 tokens down to ~300 tokens per turn).
- **Actionable Rule for Agents:**
  *Avoid broad `%functions%` catalogs in tasks where the target tool set is known upfront. Use scoped roles (`%functions:tool1,tool2%`) to preserve context window, minimize latency, and improve tool selection precision.*

---

### [2026-09-16T20:15:00-04:00] Observability: Surface Governance Taint Flags in Primary Traces
- **Category:** Observability & Verification
- **Problem:**
  When untrusted workspace runbooks tainted downstream executions (`WorkspaceTainted` $\to$ `untrusted_runbook: true`), the taint flag was passed in the evaluator JSON payload. However, without `--dialog`, standard terminal trace output on `/dev/tty` omitted the taint tag.
- **Consequence:**
  Automated test assertions verifying taint tracking had to rely on weak fallbacks (e.g. `or $file_written`), creating an illusion of test verification while masking that the taint was invisible to operators.
- **Resolution:**
  Added `untrusted_runbook: bool` to `AgentLoopEvent::RiskAssessmentStart`. In `format_trace_event`, tainted assessments explicitly render:
  `assess-risk: evaluating <tool> with <model> (untrusted_runbook: true)`
  Removed masking assertion fallbacks in `scripts/run-demos.nu`.
- **Actionable Rule for Agents:**
  *Never bury critical safety flags exclusively in debug-only or dialog-only payloads. Governance state (taint, heightening of scrutiny, policy raises) must be scannable in the primary execution trace.*

---

### [2026-09-16T20:15:00-04:00] Nushell Subprocess Captures vs. Interactive `/dev/tty` Output
- **Category:** Testing & Nushell Harnesses
- **Problem:**
  `aichat` writes real-time trace events directly to the controlling terminal (`/dev/tty`) via `write_atomic_terminal_output` to prevent trace buffering and guarantee visibility regardless of stdout/stderr pipe state. When running tests inside Nushell using `complete` (`do { ^aichat ... } | complete`), `$completed.stderr` is empty because output went directly to `/dev/tty`.
- **Consequence:**
  Nushell test assertions checking `($clean_trace | str contains "...")` fail unexpectedly during interactive terminal runs unless they account for TTY routing.
- **Resolution:**
  Following the existing convention established in Demos 1, 3, and 11, assertions checking trace events should incorporate `$trace_visually_printed = ($clean_trace | is-empty)` for interactive TTY executions, while maintaining strict string checks when stderr is redirected (CI / non-interactive).
- **Actionable Rule for Agents:**
  *When writing or debugging harness tests for binaries that write to `/dev/tty`, distinguish between captured stream content and visual terminal output to avoid false assertion failures.*

---

### [2026-09-16T20:15:00-04:00] Workspace Sandbox Containment & Path Consistency
- **Category:** Sandboxing & Integration Testing
- **Problem:**
  In Demo 23, the workspace skill was located in `/tmp/aichat-skill-ws-<pid>/.kiro/skills/repo_patcher/SKILL.md`, but the patch log target file was placed in `/tmp/aichat-patch-log-<pid>.txt` (outside the workspace). Furthermore, the runbook instructed the model to report `from <workspace_dir>`, while the prompt specified writing to the log file.
- **Consequence:**
  The workspace skill escaped its sandboxed workspace root to write a file in `/tmp`, and the trace output displayed the workspace directory instead of the patch log file referenced in the prompt.
- **Resolution:**
  Confined all artifacts inside the workspace (`$d23_ws | path join "patch.log"`), and harmonized the prompt, runbook instructions, and terminal output to reference the identical target file path.
- **Actionable Rule for Agents:**
  *Workspace skills must never write artifacts outside their designated workspace directory. Ensure prompts, runbooks, and expected outputs reference consistent, fully contained paths.*

---

### [2026-09-16T21:12:00-04:00] Parallel Tool Plan Step Collision: Match Pending Steps Before Reusing Active Step
- **Category:** Planning & Parallel Tool Execution
- **Problem:**
  In `PlanTracker::update_active_step`, the tracker previously checked if `self.active_step_id` matched `tool_name` before searching for pending steps. In parallel execution where an agent executed multiple concurrent calls to the same tool (e.g. calling `slow_task` 3 times concurrently), call 1 set step 1 to `InProgress`. Calls 2 and 3 saw `active_step_id` already matching `slow_task`, immediately returning step 1 and repeatedly emitting `[▶] 1. ...` three times while steps 2 and 3 remained `Pending`.
- **Consequence:**
  Ordered structured plans for parallel batches collapsed onto the first step, misrepresenting execution progress in traces and UI plan artifacts.
- **Resolution:**
  In `src/agent_loop/plan.rs`, prioritize searching for the first `Pending` step matching `tool_name` *first*. Only when no pending steps match the tool does the tracker fall back to keeping the currently active step (accommodating retries or multi-invocation steps).
- **Actionable Rule for Agents:**
  *When reconciling concurrent tool dispatches with an ordered plan, always advance to the next unstarted step matching the tool before assuming consecutive calls belong to the same active step.*

---

### [2026-09-16T21:12:00-04:00] Governance Gate Event Decoupling: Never Suppress Remediation or Policy Events When Deferring Verdicts
- **Category:** Safety & Governance
- **Problem:**
  To defer `SafetyGatePassed` (`ALLOW`) until after dynamic `%assess-risk%` completed, `gate_progress` was passed as `None` to `authority_denied_result`. Passing `None` inadvertently silenced all other informational governance events: `PolicyRuleMatched` (policy rules) and `PreflightReversibilityApplied` (atomic pre-mutation backups).
- **Consequence:**
  In Demo 18 (*Pre-flight Opportunistic Remediation*), the engine successfully executed an atomic backup to step down authority, but the trace completely omitted `preflight remediation: fs_write (via backup -> stepped down to reversible)`, making remediation invisible to operators and automated checks.
- **Resolution:**
  Decouple gate outcome events from verdict emissions with an explicit `defer_safety_gate: bool` parameter. Pass `progress` unconditionally: `PolicyRuleMatched` and `PreflightReversibilityApplied` emit immediately when triggered, while `SafetyGatePassed` is deferred until after the dynamic risk evaluator completes.
- **Actionable Rule for Agents:**
  *Distinguish between informational governance events (policy rules, preflight remediation) and final authorization verdicts (`ALLOW`). Never silence the entire event channel just to defer the verdict.*

---

### [2026-09-16T21:12:00-04:00] Sub-Agent Permission Provisioning: Both Mask and Ceiling Are Mandatory for Mutating Delegation
- **Category:** Multi-Agent Coordination & Delegation
- **Problem:**
  When delegating tasks to sub-agents requiring mutating tools, instructing the parent orchestrator to re-delegate "with mutating permissions" prompted the LLM to pass only `permissions_mask: "mutating"`, omitting `permissions_ceiling`. Under strict fail-closed validation (`src/function.rs`), omitted ceilings clamp to `AuthorityCeiling::MINIMAL` (`safe`). Because mutating tools (`fs_write`, `fs_create`) require at least `reversible` or `disruptive` authority, the sub-agent was trapped in an unusable state where every mutating tool failed with `risk disruptive > ceiling safe`.
- **Consequence:**
  Sub-agent delegation in Demo 21 hit repeated permission blocks and tripped circuit breakers. Furthermore, `AgentLoopEvent::CapabilityBlocked` unconditionally formatted as `"read-only mask (mutating tool; unwound: true)"`, mislabeling an authority ceiling violation as a capability mask block.
- **Resolution:**
  1. Updated delegation prompts in `scripts/run-demos.nu` to instruct re-delegation with both `permissions_mask 'mutating'` and `permissions_ceiling 'disruptive'`.
  2. Enriched `AgentLoopEvent::CapabilityBlocked` with a `reason: String` field, formatting ceiling blocks accurately as `authority ceiling exceeded (unwound: true)`.
  3. Expanded test assertions to accept any mutating tool (`fs_create` or `fs_write`) blocked during the initial probe.
- **Actionable Rule for Agents:**
  *A capability mask alone is insufficient for actuation. Sub-agents require BOTH capability mask ('mutating') and authority ceiling ('disruptive') to execute state-changing tools.*

---

### [2026-09-16T22:15:00-04:00] Tracing: Surface Model Invocation on Turn Start Across Standard and Scoped Traces
- **Category:** Observability & Trace Ergonomics
- **Problem:**
  While `--dialog` displayed the invoked model in its header boxes, the standard real-time trace line emitted on turn start (`[turn X/Y] starting`) omitted the active model name. When troubleshooting or verifying runs without full dialog traces enabled, operators could not verify which LLM was driving the turn.
- **Consequence:**
  Operators running multi-model or fall-through configurations could not inspect model allocations on `/dev/tty` without turning on verbose dialog dumps.
- **Resolution:**
  Added `model: Option<String>` to `AgentLoopEvent::TurnStart`. In `format_trace_event_styled`, turn start now renders:
  `[<agent_label> <pid> (<petname>) @ <model> [turn X/Y] starting]`
  with `@ <model>` styled in Yellow when terminal styling is enabled, matching `--dialog` conventions across all agents and scoped functional roles.
- **Actionable Rule for Agents:**
  *Observability traces must present the execution identity (agent, process, petname) AND the computational engine (model) at every turn boundary.*

---

### [2026-09-16T22:15:00-04:00] Observability: Unescape Evaluator Tool Scripts & Commands in Dialog Traces
- **Category:** Safety & Evaluator Observability
- **Problem:**
  When `%assess-risk%` evaluated tool scripts (`source`), helper scripts (`helpers`), or command arguments (`arguments.command`), `--dialog` displayed the raw JSON payload passed over the wire. Because JSON encodes newlines as `\n` and quotes as `\"`, 50-to-100-line shell scripts (including inline `awk`, conditionals, and loops) were crammed into escaped, single-line JSON string literals that were nearly impossible for humans to audit.
- **Consequence:**
  Security auditors and operators inspecting `%assess-risk%` prompts could not read the script code being evaluated for safety risk without copying and manually decoding JSON literals.
- **Resolution:**
  Implemented `pretty_format_evaluator_context` and `format_evaluator_dialog_prompt` in `src/agent_loop.rs`. The dialog display extracts `source`, `helpers`, and command arguments out of the JSON metadata, presenting the clean metadata object followed by unescaped, syntax-highlighted Markdown code blocks (````bash ... ````). The raw byte-for-byte JSON payload sent to the LLM over the wire remains completely untouched.
- **Actionable Rule for Agents:**
  *Never present escaped string literals to human auditors in diagnostic interfaces when displaying executable code. Pretty-format code into unescaped, language-tagged Markdown blocks for display while strictly preserving wire contracts for the model.*

---

### [2026-09-16T22:20:00-04:00] Harness Formatting: Isolate & Highlight User Prompts from CLI Configuration
- **Category:** Test Harnesses & Terminal Ergonomics
- **Problem:**
  In `scripts/run-demos.nu`, the demo header printed the entire command line (all environment variable overrides, flags, role arguments, and the full prompt string) on a single unsegmented line in dimmed white text. On prompts with 150+ characters, operators could not quickly distinguish between execution flags and the task instruction given to the agent.
- **Consequence:**
  Mental friction when scanning test logs and determining what behavior was requested of the agent versus which safety policies or environment flags were active.
- **Resolution:**
  In `show-cmd` ([`scripts/run-demos.nu`](file:///home/istari/projects/perry/scripts/run-demos.nu)), separate trailing prompt arguments from CLI flags and environment variables. Render the command line (`▶ AICHAT_MODEL=... aichat <flags>`) first, followed by an empty line, the user prompt isolated on its own line in highlighted `light_cyan` with a 4-space indent, followed by an empty line before execution traces begin.
- **Actionable Rule for Agents:**
  *Format test and demo harness output with clean visual separation between command-line switches/environment variables and the conversational user prompt.*

---

### [2026-09-16T22:50:00-04:00] Tracing: Upfront Token Attribution in Trace Lines and Dialog Frames
- **Category:** Observability & Trace Forensics
- **Problem:**
  While `--dialog` displayed full message payloads and turn counts, trace lines (`TurnStart`) and dialog headers only attributed the model (`@ <model>`), omitting the estimated token weight of the request. Operators evaluating context usage, token inflation across multi-turn loops, or comparing LLM invocations had to rely solely on post-turn cost reports rather than observing context size in real time.
- **Consequence:**
  Context bloat (such as massive system prompts, bulky tool returns, or runaway multi-turn history) remained invisible until the final turn or session cost summary, obscuring exactly which turn or tool result triggered high token consumption.
- **Resolution:**
  In [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs), compute the estimated token count upfront for each turn using `model.total_tokens(&msgs)` (via `estimate_token_length`) and pass `tokens: Option<usize>` into both [`AgentLoopEvent::TurnStart`](file:///home/istari/projects/perry/src/agent_loop.rs#L65-L70) and [`AgentLoopEvent::DialogBlock`](file:///home/istari/projects/perry/src/agent_loop.rs#L71-L80). Render `{tok} tok` in `DarkGray` immediately preceding ` @ <model>` in both trace headers (`[%agent% <pid> (<petname>) 286 tok @ <model> [turn X/Y] starting]`) and dialog frames (`┌── 📥 [<pid> %agent% 286 tok @ <model> [turn X/Y] PROMPT SUBMITTED TO LLM]`).
- **Actionable Rule for Agents:**
  *Compute and display request token estimates at the boundary of every LLM interaction before network dispatch. Upfront token attribution gives instant observability into turn-by-turn context growth.*

---

### [2026-09-16T23:25:00-04:00] Multi-Tool Isolation: Scoping Safety Governance Per-Subcommand, Never Per-Script
- **Category:** Safety Governance & Schema Architecture
- **Problem:**
  Agents frequently group multiple domain-specific subcommands inside a single multi-tool script (e.g. `agents/<agent>/tools.sh`). If safety governance metadata (`mode`, `risk`, `reversible-via`) were assigned at the file level, it would present an unacceptable security tradeoff: labeling `tools.sh` as `safe` would allow destructive subcommands (e.g. `clear_todos`, file overwrites) to bypass authorization gates, while labeling it `disruptive` would falsely block read-only queries (e.g. `list_todos`, `read_query`) under `safe` ceilings.
- **Consequence:**
  Either least-privilege enforcement fails (privilege escalation) or agent autonomy is unnecessarily strangled (over-conservative denial).
- **Resolution:**
  Enforce safety metadata strictly per-subcommand (`# @cmd`). During schema extraction, `argc --argc-export` parses each subcommand into its own independent entry in `functions.json`. During runtime safety evaluation, `aichat` detects multi-tool scripts (`resolved_path.file_stem() == "tools"`) and calls `extract_shell_function` to slice out *only* the invoked function's source and doc-comments, completely isolating it from sibling commands in the same file.
- **Actionable Rule for Agents:**
  *Never treat multi-tool scripts as monolithic units of risk. Always annotate each `# @cmd` subcommand with its own fine-grained `# @meta` tags.*

---

### [2026-09-16T23:30:00-04:00] Pre-Flight Remediation: File Creation Is Inherently Reversible via Undo Journaling
- **Category:** Safety & Reversibility Mechanics
- **Problem:**
  During the initial rollout of blast-radius annotations (Backlog #6b), tools that overwrite or patch existing files (`fs_write`, `fs_patch`) were annotated with `# @meta reversible-via backup`, but file creation tools like `fs_create` in `agents/coder/tools.sh` only received `# @meta mode mutating` and `# @meta risk disruptive`. The omission stemmed from a misconception that "backup" reversibility only applies when an existing file's bytes can be snapshotted prior to modification.
- **Consequence:**
  When an autonomous agent (like `coder`) runs under a `reversible` ceiling (e.g. default ceiling or delegated authority), `fs_create` tripped `authority_exceeded` because static `disruptive` could not be stepped down, even though creating a new file is trivially reversible.
- **Resolution:**
  The pre-flight remediation engine (`record_pre_mutation_journal_entry` in `src/agent_loop.rs`) already natively handles both cases:
  1. *File exists:* Snapshots existing contents to `.bak` in the `RollbackJournal` (undo restores original bytes).
  2. *File does not exist:* Records an explicit deletion command `rm -f '<path>'` in the `RollbackJournal` (undo removes the created file).
  Annotated `fs_create` in `agents/coder/tools.sh` with `# @meta reversible-via backup` and recompiled `agents/coder/functions.json`.
- **Actionable Rule for Agents:**
  *File creation tools qualify for `# @meta reversible-via backup` identically to file modification tools. The rollback journal automatically distinguishes between file snapshots and deletion tombstones.*

---

### [2026-09-16T23:31:00-04:00] Schema Synchronization: Compiled Function Declarations as Build Artifacts
- **Category:** Tool Engineering & Build Ergonomics
- **Problem:**
  In `llm-functions`, modifying doc-comments (`# @meta`, `# @option`, `# @cmd`) in `tools.sh` or `tools/*.sh` does not immediately update runtime agent schemas. Furthermore, `functions.json` is gitignored, meaning `git status` will show modified shell scripts but will not indicate whether declaration artifacts were rebuilt.
- **Consequence:**
  Developers or agents editing shell tool comments may believe the engine will pick up new metadata immediately, leading to confusing discrepancies where runtime execution fails because the active `functions.json` contains stale metadata.
- **Resolution:**
  Always run `argc build@agent <name>` (or `argc build`) after modifying tool doc-comments to re-export `.subcommands` and update `functions.json`. Remember that shell scripts are the tracked source of truth in git, while `functions.json` is a generated runtime artifact.
- **Actionable Rule for Agents:**
  *Always synchronize compiled schemas via `argc build` or `argc build@agent <name>` immediately after altering tool comment metadata.*

---

### [2026-09-21T14:25:00-04:00] Test Concurrency: Environment Variable Mutexes Require Serial Execution
- **Category:** Testing & Concurrency
- **Problem:**
  The test suite contains over 560 unit and integration tests. Several tests validate authority ceilings, capability masks, and escalation depths by mutating process-wide environment variables (`AICHAT_AUTHORITY_CEILING`, `AICHAT_AGENT_DEPTH`, etc.) guarded by `MASK_ENV_LOCK`. Running the entire test suite in parallel via standard `cargo test` allows concurrent threads reading environment variables outside the lock to race or block in futex waits.
- **Consequence:**
  Running unconstrained parallel `cargo test` across all modules intermittently blocks or stalls tests indefinitely on mutex locks.
- **Resolution:**
  When executing the full suite of unit and integration tests covering environment-sensitive safety gates, execute tests serially using `cargo test -- --test-threads=1`.
- **Actionable Rule for Agents:**
  *When validating the entire test suite in CI or local verification, use `cargo test -- --test-threads=1` to guarantee deterministic execution of environment-guarded safety tests.*

---

### [2026-09-21T16:30:00-04:00] Dedicated Baseline Identity vs. Per-Actuator Probing
- **Category:** Telemetry & Host Invariance
- **Problem:**
  Embedding host baseline discovery (hostname, OS, kernel, CPU, RAM) into every individual actuator duplicated parsing logic across tools, bloated payload sizes, and caused metric interpretation in a vacuum (e.g. reporting 26% CPU or 4GB RAM without knowing total capacity).
- **Consequence:**
  Inconsistent baseline representations, wasted LLM context on repeated static facts, and fragility across diverse architectures (x86_64, aarch64, cgroups v1/v2).
- **Resolution:**
  Extracted host discovery into a dedicated `host_env` actuator adhering to 10–15+ year portability invariants (pure integer `/proc/uptime`, 4-tier CPU resolution, multi-tier OS release parsing, cgroup memory limits).
- **Actionable Rule for Agents:**
  *Never duplicate static host baseline discovery across operational telemetry actuators. Isolate identity into a dedicated canonical actuator (`host_env`) and anchor dynamic telemetry to that baseline.*

---

### [2026-09-21T16:55:00-04:00] Multi-Pillar Turn Budgeting: Parallel Subagent Delegation Eliminates Turn Exhaustion
- **Category:** Multi-Agent Orchestration & Turn Budgeting
- **Problem:**
  When an orchestrator or top-level agent executes a comprehensive 5-pillar health audit sequentially, consuming turns for skill reading, planning, and each individual tool call exhausts tight turn limits (e.g. 5–6 turns) before the final terminal summary can be emitted.
- **Consequence:**
  The agent runs out of turns (`budget exhausted at N turns`), failing test assertions and dropping the final diagnostic report.
- **Resolution:**
  Prescribe parallel execution in runbooks and delegate independent pillars to specialist subagents (e.g. `sre`) in parallel. Perry's `join_all` runs all subagent subprocesses concurrently in a single turn. The orchestrator needs only 4 turns total: turn 1 `read_skill`, turn 2 `_plan`, turn 3 parallel dispatch of 5 `sre` subagents, turn 4 synthesis.
- **Actionable Rule for Agents:**
  *In multi-pillar audits, never execute independent checks sequentially in the root loop. Formulate an upfront plan and delegate independent pillars to specialist subagents concurrently in a single turn.*

---

### [2026-09-21T17:05:00-04:00] Dynamic Semantic Key-Value Extraction vs. Rigid Schema Enums
- **Category:** Telemetry Distillation & Evidence Extraction
- **Problem:**
  Attempting to map arbitrary infrastructure telemetry (log traces, socket anomalies, kernel errors) into rigid, hardcoded JSON schemas or enum fields causes lossy truncation of unpredictable diagnostic attributes.
- **Consequence:**
  Critical troubleshooting values (PIDs, failure steps, interface drops, error messages) are dropped or forced into generic fallback strings.
- **Resolution:**
  The decoupled distillation tap (`%distill-telemetry%`) instructs the evaluator LLM to extract key-values semantically and dynamically without assuming a fixed schema, preserving verbatim technical evidence and materializing log queries and dumps as clickable `file://` hyperlinks.
- **Actionable Rule for Agents:**
  *Do not force open-ended system diagnostic evidence into rigid schema enums. Employ dynamic semantic key-value extraction to discover diagnostic keys organically from live system outputs.*

---

### [2026-09-22T14:10:00-04:00] Config Directory Decoupling & Dual Subprocess Interoperability
- **Category:** Configuration & Process Environment
- **Problem:**
  When transitioning a codebase from upstream (`aichat`) to a dedicated fork namespace (`perry`), the engine's default configuration path moved from `~/.config/aichat/` to `~/.config/perry/`. However, existing actuator tool scripts, child tools, or companion wrappers often invoke `/usr/bin/aichat` or look for upstream configuration paths, causing them to fall back to stale or missing credentials if only `~/.config/perry/` is populated.
- **Consequence:**
  Child tools executed by Perry failed due to missing configuration or prompted interactively for configuration creation, breaking non-interactive agent execution.
- **Resolution:**
  Established a dedicated `~/.config/perry/` directory (with `functions/` symlinked to `~/projects/innators`). In [`src/function.rs`](file:///home/istari/projects/perry/src/function.rs), Perry's `eval_shell` automatically injects both `PERRY_CONFIG_DIR` and `AICHAT_CONFIG_DIR` pointing to the resolved Perry config directory into child tool process environments.
- **Actionable Rule for Agents:**
  *When migrating or decoupling a CLI fork's configuration path, inject dual config directory environment pointers into subprocesses and tool wrappers so that legacy actuator scripts seamlessly read the new configuration root without breaking.*

---

### [2026-09-22T14:15:00-04:00] Default Autonomy Posture: Engine-Level Principle of Least Privilege
- **Category:** Safety & Autonomy Ladder
- **Problem:**
  Without an explicit `--autonomy` flag, the agent loop previously defaulted to an unconstrained operational posture (`autonomy: None`), leaving the root process Gate 2 authority ceiling at `Destructive`. This violated the principle of least privilege: an operator running `perry --agent sre` could unintentionally execute destructive commands without realizing that unconstrained autonomy was active.
- **Consequence:**
  Automated agent invocations lacked a safe baseline posture by default, requiring operators to remember to pass `--autonomy readonly` on every invocation.
- **Resolution:**
  Implemented Method 2: Made `--autonomy readonly` the engine-level default in `SafetyConfig`. Unconstrained execution now requires an explicit opt-out (`--autonomy none` or `unrestricted`). Fine-grained authority overrides (e.g. `--ceiling` or role-specific ceilings) remain strictly isolated, and unit tests verify that macro postures do not accidentally bleed across isolated gate tests.
- **Actionable Rule for Agents:**
  *Default autonomous agent loops to the safest operational posture (`readonly`). Provide an explicit opt-out (`--autonomy none`) rather than making unconstrained execution the silent default.*

---

### [2026-09-22T14:35:00-04:00] Actuator Tool Environment Dependencies & Hierarchical Config Fallbacks
- **Category:** Actuator Governance & Configuration
- **Problem:**
  External tool scripts defined with `argc` mandatory environment variables (e.g. `# @env WEB_SEARCH_MODEL!`) failed immediately with non-zero exit codes if the environment variable was not explicitly exported in the parent shell, even when valid models were configured in `config.yaml`. Furthermore, Perry had no engine-level `web_search_model` key in `Config`.
- **Consequence:**
  Agents calling `web_search` crashed during tool pre-flight before `main()` could run, unless the user manually set `export WEB_SEARCH_MODEL=...` beforehand.
- **Resolution:**
  1. Relaxed `argc` declaration in `web_search_perry.sh` (aliased as `web_search_aichat.sh`) from required (`!`) to optional (`# @env WEB_SEARCH_MODEL`).
  2. Implemented hierarchical resolution in both the shell tool and Perry core engine:
     `WEB_SEARCH_MODEL` (env) $\to$ `web_search_model` (`config.yaml`) $\to$ `model` (primary `config.yaml` model) $\to$ default (`gemini:gemini-2.5-flash`).
  3. Perry's `eval_shell` automatically injects `WEB_SEARCH_MODEL` into tool process environments if unset.
- **Actionable Rule for Agents:**
  *Never make environment variables mandatory (`!`) in actuator scripts if sensible defaults or configuration file fallbacks can be resolved. Support a hierarchical resolution order: explicit env var $\to$ specific config key $\to$ general config model $\to$ stable default.*

---

### [2026-09-23T11:45:00-04:00] Anti-Pattern: Global Permission Relaxation in Test Harnesses Masks Multi-Gate Governance
- **Category:** Testing, Safety & Governance Architecture
- **Problem:**
  When `--autonomy readonly` was made the engine-level default in Session 31, mutating and delegation demo tests broke in `scripts/run-demos.nu`. An initial proposal suggested injecting `PERRY_AUTONOMY: "none"` globally into `base_env`.
- **Consequence:**
  1. *Erosion of Default Posture Verification:* Applying `PERRY_AUTONOMY: "none"` across the entire harness completely disables testing of the default least-privilege CLI posture (`--autonomy readonly`). Over 70% of tests (read-only SRE diagnostics, web research, structured parsing) would no longer verify that out-of-the-box unprivileged execution works cleanly.
  2. *Violation of Least Privilege in Mutating Tests:* Demos needing only `reversible` authority (e.g. `generate_data` under `# @meta risk reversible`) would run with full unconstrained destructive authority, making the test far more permissive than necessary.
  3. *Short-Circuiting Multi-Gate Safety Defenses (False Positives/Negatives):* In Perry's governance pipeline, Gate 1 (macro capability mask) operates orthogonally to Gate 2 (dynamic risk evaluation and authority ceilings). Running mutating safety tests under default `readonly` caused Gate 1 to fail closed (`capability_denied`) before Gates 2–5 could execute. For example, Demo 19 (`fs_write` vs `ceiling: safe`) is intended to test that `reversible > safe` fails closed at Gate 2; under `readonly`, it was blocked at Gate 1, creating a false-positive pass for the wrong reason. Conversely, globally disabling autonomy removes Gate 1 entirely instead of scoping permits to the exact gate being evaluated.
- **Resolution:**
  Never use global blanket permission relaxations in test harnesses. Instead, apply targeted, per-test flags adhering to the Principle of Least Privilege:
  - Standard read-only and diagnostic tests run under the default `--autonomy readonly` with zero override flags.
  - Reversible mutation tests explicitly pass `--autonomy reversible`.
  - Specific dynamic policy and delegation tests (e.g. testing `arg_contains: "rm -rf"` elevating to `catastrophic` at Gate 2, or testing orchestrator downward sandboxing) pass `--autonomy none` strictly on the specific command under test, while preserving child sandbox boundaries.
- **Actionable Rule for Agents:**
  *Never propose global permission relaxations (e.g. setting `PERRY_AUTONOMY: "none"` in a base environment) to fix test failures. Always diagnose the exact gate failure (Gate 1 capability mask vs Gate 2 authority ceiling vs Gate 3 approval floor) and apply minimal, per-test flags adhering to the Principle of Least Privilege.*

---

### [2026-09-23T12:35:00-04:00] Autonomy Ladder Symmetry: Blast-Radius Tiers Over Ambiguous Permission Labels
- **Category:** Safety Taxonomy & Ergonomics
- **Problem:**
  The Autonomy Ladder initially exposed only three macro presets (`readonly`, `consult`, `reversible`), while using `none` or `unrestricted` to clear the macro and fall back to `safety.default_ceiling` (`Destructive`). This introduced two semantic hazards:
  1. *The "None" Inversion:* `--autonomy none` sounded like "zero autonomy", but in practice granted raw unconstrained autonomy up to `Destructive`.
  2. *The "Unrestricted" False Promise:* Calling the top tier "unrestricted" implied that anything goes, including catastrophic infrastructure destruction (e.g. `rm -rf /` or dropping root DBs). However, in Perry, `Catastrophic` is **always reserved for humans** (`RequiredAuthority::Human`) and can never be permitted to an autonomous process.
  3. *The Missing Disruptive Step:* Intermediate operations (e.g. restarting daemons or flushing caches) had no dedicated macro preset, leaving an awkward gap between `reversible` and `destructive`.
- **Consequence:**
  Operators could either be surprised by over-permissive behavior (`none`), misled about safety bounds (`unrestricted`), or forced into manual fine-grained environment variable overrides for intermediate operational tasks.
- **Resolution:**
  Harmonized the Autonomy Ladder to directly match the 5 canonical Blast-Radius Tiers for complete symmetry and honesty:
  - `readonly` (A0): Read-only triage and inspection ($0 LLM evaluator cost, Gate 1 hard block).
  - `consult` (A1): Supervised copilot (Gate 1 unmasked; all mutations evaluated and prompt the operator).
  - `reversible` (A2): Bounded autopilot (Option B atomic `.bak` journal step-down enabled).
  - `disruptive` (A3): Active SRE remediation (service restarts, pod rollouts, and cache flushes auto-execute within ceiling).
  - `destructive` (A4): Maximum autonomous actuation (deletions auto-execute within ceiling; **catastrophic remains strictly human-reserved**).
  - `off` / `none` / `raw`: Explicit macro opt-out to rely on raw configuration and environment variables.
- **Actionable Rule for Agents:**
  *Never use misleading or inverse permission names like 'unrestricted' when hard safety invariants (such as Catastrophic human reservations) remain active. Maintain exact terminology symmetry between operational macros and underlying blast-radius tiers.*

---

### [2026-09-23T16:45:00-04:00] Actuator Observability Decoupling & Scoped Agent Tooling
- **Category:** Tooling & Agent Decomposition
- **Problem:**
  1. Actuators declared `# @meta require-tools perry|aichat` assuming `argc` supported alternative binary dependencies. Instead, `argc` treated the pipe as a literal binary name (`perry|aichat`), failing preflight for `web_search_perry.sh` and `summarize_text.sh`.
  2. In `host_env.sh`, sub-actions (`os()`, `hardware()`, etc.) piped `summary | jq ...`. When `--dialog` was enabled, `summary` called `_emit_raw` to echo human-readable bash commands to `stdout`, injecting non-JSON text into the pipe and crashing `jq` with `parse error: Invalid numeric literal`.
  3. The `sre` agent lacked basic read-only file reading tools (`fs_cat`, `fs_ls`), causing orchestrator delegation to route configuration reads (e.g. `/etc/os-release`) through `host_env --action os`.
- **Consequence:**
  Actuator preflight failed for all web search queries, and `--dialog` runs crashed downstream JSON parsing and tripped the circuit breaker.
- **Resolution:**
  1. Removed `perry|aichat` from `@meta require-tools`, handling CLI presence checks and runner fallbacks natively within bash `main()`.
  2. Extracted JSON assembly in `host_env.sh` into `_get_summary_json()` which emits pure JSON without calling `_emit_raw`, cleanly separating stdout observability trace streams from data pipes.
  3. Equipped the `sre` agent with safe, read-only `fs_cat.sh` and `fs_ls.sh` actuators and updated system prompts to explicitly support configuration reviews.
- **Actionable Rule for Agents:**
  *Never pipe functions that write human-readable diagnostics (`_emit_raw`) to structured data parsers (`jq`). Decouple data gathering from trace rendering. Do not use logical operators in `argc` `@meta require-tools` headers.*

---

### [2026-09-24T17:15:00-04:00] Inversion of Authority: Subagents Must Report Diagnostic Telemetry, Never Instruct the Supervisor on Permissions
- **Category:** Safety Architecture, Supervisory Control & Delegation
- **Problem:**
  When a provisioned subagent was blocked at Gate 2 because an actuator required an authority ceiling exceeding the subagent's ceiling (e.g., `fs_write` requiring `reversible` when spawned with `safe`), the harness error string previously coached the caller with prescriptive guidance:
  `"To permit this mutation, re-delegate to '{agent}' with permissions: { mask: ..., ceiling: ... } (if authorized and within your ceiling)."`
- **Consequence:**
  1. *Confused Deputy & Inversion of Authority:* In multi-tier agent hierarchies (e.g. `orchestrator` $\to$ `coder`), an unprivileged subagent's execution environment effectively instructed its supervisor to escalate privileges. This inverts the principle of supervisory control—privilege escalation requests should originate from operational necessity evaluated by the supervisor or a human, not suggested upward by the constrained child process.
  2. *LLM Paralyzing Indecision:* When the LLM orchestrator saw `"If this action is authorized and within your ceiling"`, it interpreted this conditional hedge as doubt or a warning that it should not re-delegate, frequently abandoning execution instead of completing valid operational tasks (observed in Demo 21).
- **Resolution:**
  Replaced prescriptive coaching strings with objective, factual diagnostic telemetry:
  `"Sub-agent '{agent}' was blocked because tool '{tool}' requires authority ceiling '{ceiling}', which exceeds its provisioned ceiling. Pre-mutation entries were unwound."`
  Updated the Orchestrator's system instructions in `innators/agents/orchestrator/AGENT.md` to autonomously evaluate `permission_blocked` failures:
  - The orchestrator inspects the blocked tool, its required ceiling, and whether the mutation is strictly necessary and within its own ceiling.
  - If necessary and authorized, the orchestrator re-delegates with the necessary ceiling.
  - If beyond its own authority or policy, it escalates to the human operator.
- **Actionable Rule for Agents:**
  *Never allow child subagent error messages or tool denial telemetry to prescribe permission escalation syntax or nudge supervisors. Downward authority must remain strictly top-down: report objective denial facts (`EPERM` diagnostics) and leave authority assessment and delegation decisions entirely to the supervisor.*

