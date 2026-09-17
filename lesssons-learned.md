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
  In `show-cmd` ([`scripts/run-demos.nu`](file:///home/istari/projects/aichat/scripts/run-demos.nu)), separate trailing prompt arguments from CLI flags and environment variables. Render the command line (`▶ AICHAT_MODEL=... aichat <flags>`) first, followed by an empty line, the user prompt isolated on its own line in highlighted `light_cyan` with a 4-space indent, followed by an empty line before execution traces begin.
- **Actionable Rule for Agents:**
### [2026-09-16T22:50:00-04:00] Tracing: Upfront Token Attribution in Trace Lines and Dialog Frames
- **Category:** Observability & Trace Forensics
- **Problem:**
  While `--dialog` displayed full message payloads and turn counts, trace lines (`TurnStart`) and dialog headers only attributed the model (`@ <model>`), omitting the estimated token weight of the request. Operators evaluating context usage, token inflation across multi-turn loops, or comparing LLM invocations had to rely solely on post-turn cost reports rather than observing context size in real time.
- **Consequence:**
  Context bloat (such as massive system prompts, bulky tool returns, or runaway multi-turn history) remained invisible until the final turn or session cost summary, obscuring exactly which turn or tool result triggered high token consumption.
- **Resolution:**
  In [`src/agent_loop.rs`](file:///home/istari/projects/aichat/src/agent_loop.rs), compute the estimated token count upfront for each turn using `model.total_tokens(&msgs)` (via `estimate_token_length`) and pass `tokens: Option<usize>` into both [`AgentLoopEvent::TurnStart`](file:///home/istari/projects/aichat/src/agent_loop.rs#L65-L70) and [`AgentLoopEvent::DialogBlock`](file:///home/istari/projects/aichat/src/agent_loop.rs#L71-L80). Render `{tok} tok` in `DarkGray` immediately preceding ` @ <model>` in both trace headers (`[%agent% <pid> (<petname>) 286 tok @ <model> [turn X/Y] starting]`) and dialog frames (`┌── 📥 [<pid> %agent% 286 tok @ <model> [turn X/Y] PROMPT SUBMITTED TO LLM]`).
- **Actionable Rule for Agents:**
  *Compute and display request token estimates at the boundary of every LLM interaction before network dispatch. Upfront token attribution gives instant observability into turn-by-turn context growth.*
