# Project Perry: Canonical Register of Architectural & Development Anti-Patterns (DON'Ts)

> **Version:** 1.0.0  
> **Status:** Canonical & Enforced  
> **Target Audience:** Human maintainers, system operators, and AI agents collaborating on the Perry codebase.  
> **Mandate:** Any agent operating in this repository must strictly adhere to these architectural and development prohibitions. Every entry represents a ground-truthed invariant established across Sessions 1 through 31 (2026-08-26 to 2026-09-22) to prevent regressions, deadlocks, privilege escalations, and context blowouts.

---

## Table of Contents
1. [Chronological Overview of Development Eras](#chronological-overview-of-development-eras)
2. [Era 1: Core Agent Loop, Tool Dispatch & Capability Masking (Sessions 1–6)](#era-1-core-agent-loop-tool-dispatch--capability-masking-sessions-16)
3. [Era 2: Blast-Radius Taxonomy, Reversibility & Grounded Evaluation (Sessions 7–15)](#era-2-blast-radius-taxonomy-reversibility--grounded-evaluation-sessions-715)
4. [Era 3: Sandbox Boundaries, Grounding Control & Loop Resilience (Sessions 16–24)](#era-3-sandbox-boundaries-grounding-control--loop-resilience-sessions-1624)
5. [Era 4: Observability, Structured Plans, Taint Lifecycle & Autonomy Ladder (Sessions 25–28)](#era-4-observability-structured-plans-taint-lifecycle--autonomy-ladder-sessions-2528)
6. [Era 5: Rebranding, Host Invariance, Production Posture & Decoupling (Sessions 29–31)](#era-5-rebranding-host-invariance-production-posture--decoupling-sessions-2931)
7. [The Top 10 Non-Negotiable Invariants](#the-top-10-non-negotiable-invariants)

---

## Chronological Overview of Development Eras

| Era | Session Range | Timeframe | Primary Focus & Hard Boundaries Established |
| :--- | :--- | :--- | :--- |
| **Era 1** | Sessions 1–6 | 2026-08-26 – 2026-09-02 | Agent loop iteration, tool circuit breakers, stream routing, binary capability mask (`#6a`), framing protocol. |
| **Era 2** | Sessions 7–15 | 2026-09-04 – 2026-09-08 | 5-tier blast radius taxonomy, reversibility discounting, Protected Policy Files, `#6c` unanchored risk evaluation, Option B remediation. |
| **Era 3** | Sessions 16–24 | 2026-09-08 – 2026-09-12 | Prohibition of downward permits, process sandboxing, parallel history preservation, empty-turn cache busting, truthful error reporting. |
| **Era 4** | Sessions 25–28 | 2026-09-14 – 2026-09-17 | Async relay draining, structured planning (`#15`), step-bound skill taint tracking (`#17`), Autonomy Ladder (`#19`). |
| **Era 5** | Sessions 29–31 | 2026-09-18 – 2026-09-22 | History-preserving repository migration, 10–15+ year host portability (`host_env`), default least-privilege (`--autonomy readonly`), hierarchical fallbacks. |

---

## Era 1: Core Agent Loop, Tool Dispatch & Capability Masking (Sessions 1–6)

### 1. NEVER Let Failing Tools Thrash the Agent Loop
* **Origin:** Session Changes (2026-08-26) / Backlog #3.
* **The Failure Mode:** When an external tool binary encounters persistent errors (e.g. invalid arguments, 401 Unauthorized, host network disconnect), models frequently enter a repetitive retry loop, consuming all remaining turns in the budget without producing useful work.
* **The Prohibition:** Never allow repeated consecutive failures of the same tool within a single loop execution.
* **The Enforced Rule:** Enforce a hard **Tool Circuit Breaker** tripping on 3 consecutive failures. Once tripped, subsequent invocations immediately return `{"error": {"type": "circuit_breaker", "message": "Tool '<name>' has been disabled after 3 consecutive failures"}}` without executing the binary. A single successful execution resets the counter.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs); verified via unit test `test_circuit_breaker_trips_after_three_failures`.

---

### 2. NEVER Dump Tool Outputs Larger than 16KB Directly into LLM Context
* **Origin:** Session Changes (2026-08-26) & Session 3 (2026-09-02) / Backlog #4 (`tool-output-routing`).
* **The Failure Mode:** System administration commands (`journalctl -u ...`, `cat /var/log/syslog`, CSV exports) can produce megabytes of output. Passing this directly into the conversation history exhausts the model's context window, degrades reasoning, and causes massive token billing.
* **The Prohibition:** Never dump raw tool execution stdout into the next turn payload if it exceeds the configured threshold.
* **The Enforced Rule:** Enforce **16KB Stream Auto-Capping** (`tool_output_limit: 16384`). Tool results exceeding 16KB are spooled to `$XDG_RUNTIME_DIR/aichat-tool-<uuid>.out`. The model receives a truncated 20-line head/tail preview, the byte size, and an absolute file path pointer.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs) and [`src/config/mod.rs`](file:///home/istari/projects/perry/src/config/mod.rs); verified in `test_output_routing_auto_capping`.

---

### 3. NEVER Allow Cyclic Tool Pipe Routing
* **Origin:** Session 3 (2026-09-02) / Backlog #4 (`tool-output-routing`).
* **The Failure Mode:** Declarative tool output routing allows chaining tools (`"destination": "pipe", "target": "<tool2>"`). If tools form a cycle ($A \to B \to A$), the engine executes an unconstrained infinite loop at the process level.
* **The Prohibition:** Never dispatch tool pipes without cyclic graph traversal checks.
* **The Enforced Rule:** Maintain a visited `HashSet<String>` along the execution pipeline. If a target tool is already in the visited set, abort the chain immediately with a structured `pipe_cycle_detected` error.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs); verified in `test_pipe_routing_cycle_detection`.

---

### 4. NEVER Allow Subagents to Execute Mutating Tools by Default
* **Origin:** Session 4 (2026-09-02) / Backlog #6a (`tool-safety-modes`).
* **The Failure Mode:** When an orchestrator spawns subagents to research or inspect an environment, subagents possessing mutating tools can accidentally alter files or restart services without operator knowledge.
* **The Prohibition:** Never spawn subagents with unconstrained capability masks.
* **The Enforced Rule:** Every child process spawned via `agent: true` unconditionally inherits `AICHAT_CAPABILITY_MASK=readonly`. In `readonly` mode, Gate 1 intercepts any tool not explicitly classified as `mode: readonly` and returns `{"error": {"type": "capability_denied", "reason": "mutating"}}` without executing the binary.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs); verified in Demo 2 (`scripts/run-demos.nu`).

---

### 5. NEVER Gate the Delegation Act Itself (Decision B: Delegation $\neq$ Actuation)
* **Origin:** Session 5 (2026-09-02) / Backlog #6b (`Decision B`).
* **The Failure Mode:** Treating the act of spawning a subagent as an actuation risk blocks masked or limited agents from delegating tasks to read-only researchers, paralyzing multi-agent orchestration.
* **The Prohibition:** Never subject tool calls with `agent: true` to Gate 1 (capability mask) or Gate 2 (authority ceiling).
* **The Enforced Rule:** Spawning a subagent is orchestration, not actuation. The real risk is what the subagent executes inside its own process, which is gated by its inherited sandbox. Guard both gates with `call_targets_agent(...)` and skip them for delegation.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs#L400-L420).

---

### 6. NEVER Rely on Heavy External Transport Dependencies for Local Escalation
* **Origin:** Session 6 (2026-09-02) / Backlog #6d (`Transport Decision Path 1′`).
* **The Failure Mode:** Pulling in WebSocket crates (`tokio-tungstenite`) or HTTP server stacks for child-to-parent escalation inflates binary size, increases attack surface, and risks protocol mismatches on minimal bastions.
* **The Prohibition:** Never introduce external transport protocols for loopback supervisory IPC.
* **The Enforced Rule:** Use pure **Loopback TCP with 4-byte big-endian length-delimited JSON framing** (`write_frame` / `read_frame`). Reuses in-tree `tokio-rustls` with ephemeral in-memory DER certificates without external PKI.
* **Verification / Code Reference:** [`src/escalation.rs`](file:///home/istari/projects/perry/src/escalation.rs).

---

## Era 2: Blast-Radius Taxonomy, Reversibility & Grounded Evaluation (Sessions 7–15)

### 7. NEVER Conflate the Impact Axis with the Authority Axis
* **Origin:** Session 7 (2026-09-04) & Session 27 (2026-09-16).
* **The Failure Mode:** Using "Safe" or "Destructive" interchangeably for what an action *is* and what an agent *is allowed to do* causes type confusion and subtle security bypasses.
* **The Prohibition:** Never use a single scalar type to represent both tool blast radius and agent execution limits.
* **The Enforced Rule:** Maintain strict mathematical orthogonality:
  * **Impact Axis (`ImpactTier` / `BlastRadius`):** Intrinsic action consequence (`Safe < Reversible < Disruptive < Destructive < Catastrophic`).
  * **Authority Axis (`AuthorityCeiling`):** What an agent may actuate autonomously (`UpTo(BlastRadius)` vs `HumanReserved`).
  * **Mask Axis (`ToolMode`):** Process-level admission filter (`Readonly` vs `Full`).
* **Verification / Code Reference:** [`src/function.rs`](file:///home/istari/projects/perry/src/function.rs) and [`src/safety.rs`](file:///home/istari/projects/perry/src/safety.rs).

---

### 8. NEVER Treat `HumanReserved` as a 6th Blast-Radius Tier
* **Origin:** Session 7 (2026-09-04) & Session 27 (2026-09-16).
* **The Failure Mode:** Adding `Human` to the `BlastRadius` enum creates invalid tool metadata (`risk: human`) that corrupts impact classification.
* **The Prohibition:** Never define `Human` on the impact axis.
* **The Enforced Rule:** `BlastRadius` has exactly 5 variants. `HumanReserved` lives strictly on the **Authority Axis** in [`RequiredAuthority`](file:///home/istari/projects/perry/src/safety.rs):
  $$\text{RequiredAuthority} = \text{Tier}(\text{BlastRadius}) \;\mid\; \text{Human}$$
  Triggered exclusively by undeclared tools, `forbid: true` policy rules, catastrophic impact, or low evaluation confidence.
* **Verification / Code Reference:** [`src/safety.rs`](file:///home/istari/projects/perry/src/safety.rs#L40-L56).

---

### 9. NEVER Discount `Catastrophic` Actions via Reversibility
* **Origin:** Session 5 (2026-09-02) & Session 7 (2026-09-04) / `Catastrophic Hard Floor Exception`.
* **The Failure Mode:** A tool declaring `# @meta reversible-via backup` on a catastrophic action (e.g. wiping partition tables with a backup) could step down required authority from `Catastrophic` to `Destructive`, allowing autonomous execution under a `Destructive` ceiling without human review.
* **The Prohibition:** Never allow proven reversibility to step down a catastrophic action.
* **The Enforced Rule:** The step-down discount (`one_step_down`) applies strictly to `Disruptive` (stepping to `Reversible`) and `Destructive` (stepping to `Disruptive`). `Catastrophic` actions—whether intrinsic or policy-raised—**always require a human**.
* **Verification / Code Reference:** [`src/safety.rs`](file:///home/istari/projects/perry/src/safety.rs#L330-L335); tested in `test_reversibility_does_not_discount_catastrophic`.

---

### 10. NEVER Fail Closed on Reversible Actions Due to Pre-Mutation Timing (Option B)
* **Origin:** Session 9 (2026-09-07) / Backlog #6b (`Pre-flight Opportunistic Remediation`).
* **The Failure Mode:** The Chicken-and-Egg Safety Paradox: tools like `fs_write` declare `# @meta reversible-via backup`. However, because backups were historically made during execution, pre-flight gating saw the action as unbacked and classified it as `Disruptive`, blocking agents running under a `Reversible` ceiling.
* **The Prohibition:** Never evaluate reversibility authority solely on the host's static state prior to execution without checking pre-flight remediation capabilities.
* **The Enforced Rule:** **Option B Pre-flight Remediation:** When an action trips the ceiling solely because it is not yet proven reversible, the engine intercepts the breach, takes an atomic backup of the target file into `$XDG_RUNTIME_DIR/aichat/journals/artifacts/`, writes the undo command to the `0600 WAL`, flips `reversible: true`, and steps down required authority (`Disruptive -> Reversible`).
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs); verified in Demo 18 (`scripts/run-demos.nu`).

---

### 11. NEVER Load Protected Policy Files with Insecure Permissions
* **Origin:** Session 7 (2026-09-04) & Session 10 (2026-09-08).
* **The Failure Mode:** If `safety.toml` is world-readable or group-writable, untrusted local users can modify regex rules to grant arbitrary execution privileges or bypass human escalation.
* **The Prohibition:** Never parse or execute policy files that have group or world permissions on Unix.
* **The Enforced Rule:** On Unix systems, check metadata permissions before parsing:
  ```rust
  if mode & 0o077 != 0 {
      bail!("Protected policy file must be owner-only (0600); run chmod 600 ...");
  }
  ```
  Fail closed immediately with an error; never issue a warning.
* **Verification / Code Reference:** [`src/safety.rs`](file:///home/istari/projects/perry/src/safety.rs#L448-L465); tested in `test_policy_file_rejects_insecure_permissions`.

---

### 12. NEVER Approve Child Escalations Purely Because the Supervisor Holds Authority
* **Origin:** Session 10 (2026-09-08) / Supervisory Should Gate (`FR-6d.19`).
* **The Failure Mode:** A supervisor process possessing `Destructive` ceiling previously approved all child escalations simply because the supervisor held the ceiling ("Can == Should"), without checking whether the requested action aligned with policy or safety rules.
* **The Prohibition:** Never rubber-stamp escalations based on raw parent ceiling.
* **The Enforced Rule:** The supervisor’s `handle_escalation_request` must execute **The Should Gate**:
  1. Evaluate the supervisor's Protected Policy File (explicit `forbid: true` halts immediately).
  2. Enforce the anti-spoofed static tier and reversibility floor.
  3. Invoke supervisory `%assess-risk%` evaluation with grounded context before granting approval.
* **Verification / Code Reference:** [`src/escalation.rs`](file:///home/istari/projects/perry/src/escalation.rs); tested in `supervisory_decision_raises_over_ceiling_does_not_permit`.

---

### 13. NEVER Feed Declarative OpenAPI Schemas to the Risk Evaluator
* **Origin:** Session 13 & Session 14 (2026-09-08).
* **The Failure Mode:** Feeding the `%assess-risk%` model full JSON schemas (`type`, `properties`, parameter descriptions) cluttered the prompt with ~4,000 tokens of boilerplate. The model hallucinated risk based on schema descriptions rather than what the script actually executed.
* **The Prohibition:** Never present abstract parameter schemas to the risk evaluator.
* **The Enforced Rule:** Provide **Concrete Execution Ground Truth**:
  * Exact tool name and bound CLI command preview.
  * Runtime argument values.
  * Resolved script implementation extracted directly from source (via `extract_shell_function`).
  * Active preflight reversibility safeguards (`rollback_mechanism`).
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs); verified in Session 14 live trace inspection.

---

### 14. NEVER Leak Pre-Classified Tiers or Anchoring Directives to the Evaluator
* **Origin:** Session 15 (2026-09-08) / Unbiased Risk Assessment (`FR-6c.11`).
* **The Failure Mode:** Prompts including `static_tier: "disruptive"` and directives like "You may only make an action STRICTER" anchored the LLM's assessment, preventing it from honestly flagging subtle hazards on supposedly "safe" scripts.
* **The Prohibition:** Never expose static classifications or enforcement clamps to the evaluator LLM.
* **The Enforced Rule:** The LLM acts as an unanchored auditor that evaluates blast radius (`Safe` $\to$ `Catastrophic`) and confidence purely from execution facts. The Rust engine mathematically enforces the non-pardonable catalog and policy base floor via `clamp_verdict`. Strip `# @meta risk` tags from extracted tool source code before passing it to the evaluator.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs) and `assets/roles/%assess-risk%.md`.

---

## Era 3: Sandbox Boundaries, Grounding Control & Loop Resilience (Sessions 16–24)

### 15. NEVER Issue Downward Permit Tokens In-Flight (The Session 16 Pivot)
* **Origin:** Session 16 (2026-09-08) / Prohibition of Downward Permits (`FR-6d.24`).
* **The Failure Mode:** Under Session 14's `FR-6d.22`, when a child escalated over mTLS, the supervisor could issue an `ExecutionPermit` token (`permit-<uuid>`). The child used this token to bypass its own authority ceiling (`supervisory_approved = true`), breaking process sandboxing.
* **The Prohibition:** Never issue in-flight privilege elevation permits down the agent tree.
* **The Enforced Rule:** A child agent process's execution sandbox (`mask` and `ceiling`) is **immutable** for its entire lifetime. When an action exceeds the ceiling, the child halts, unwinds any pre-mutation journal entries via `journal.replay_last()`, and emits `permission_blocked`. The parent orchestrator must re-delegate with the required ceiling upfront or execute the tool directly.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs); verified in Demo 20 and Demo 21 (`scripts/run-demos.nu`).

---

### 16. NEVER Drop Executed Tool Results from Conversation History
* **Origin:** Session 20 (2026-09-09) / Demo 5 Infinite Loop Root Cause.
* **The Failure Mode:** In `eval_tool_calls_parallel`, code copied from legacy single-turn CLI execution dropped results:
  ```rust
  let is_all_done = results.iter().all(|r| r.output == json!("DONE"));
  if is_all_done { return Ok(vec![]); } // BUG!
  ```
  Returning an empty result vector caused the next turn's prompt to be byte-for-byte identical to the prior turn. The model assumed its tool call was ignored and repeated it 18 consecutive times until exhausting its budget.
* **The Prohibition:** Never discard or filter out executed `ToolResult` entries from the agent loop history.
* **The Enforced Rule:** Every executed tool call unconditionally preserves its `ToolResult` in conversation history, including null or `"DONE"` outputs.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs#L366-L395); verified in `test_eval_tool_calls_parallel_preserves_all_results_without_dropping`.

---

### 17. NEVER Omit `set -eo pipefail` in Shell Actuators Chaining Pipelines
* **Origin:** Session 20 (2026-09-09) / Silent Pipeline Failure Bug.
* **The Failure Mode:** `fetch_url_via_curl.sh` executed `curl ... | html-to-markdown`. When `curl` failed with HTTP 404, `html-to-markdown` exited with code 0 on empty input. Without `pipefail`, the script exited 0 with 0 bytes written to `$LLM_OUTPUT`, triggering the cascade in Don't 16.
* **The Prohibition:** Never construct multi-command shell pipelines without strict bash error flags.
* **The Enforced Rule:** All actuator scripts must start with:
  ```bash
  set -eo pipefail
  ```
  Ensure timeouts (`-m 30`) and modern User-Agent headers are set on network requests to avoid bot-blocks.
* **Verification / Code Reference:** `tools/fetch_url_via_curl.sh` in [`innators`](file:///home/istari/projects/innators).

---

### 18. NEVER Wrap URLs with Markdown Link Brackets When URLs Contain Base64 Tokens
* **Origin:** Sessions 20 & 22 (2026-09-09/11) / Grounding Token Corruption.
* **The Failure Mode:** Search engines and grounding APIs return tracking/redirect URLs containing long alphanumeric and base64 tokens. Formatting them as `[Title](URL)` caused markdown parsers and LLMs to split or truncate tokens across newlines, causing 404 redirect failures on downstream fetches.
* **The Prohibition:** Never format URLs with markdown brackets in search tool outputs intended for automated extraction.
* **The Enforced Rule:** Format search results using dedicated line key-values:
  ```text
  Title: <Page Title>
  URL: <Canonical URL>
  Summary: <Summary>
  ```
* **Verification / Code Reference:** `tools/web_search_perry.sh` (aliased as `web_search_aichat.sh`) in [`innators`](file:///home/istari/projects/innators).

---

### 19. NEVER Disguise Tool Execution Errors as Successful Completions
* **Origin:** Session 22 (2026-09-11) / Truthful Tool Failure Trace Reporting.
* **The Failure Mode:** `eval_tool_calls_parallel` emitted `AgentLoopEvent::ToolComplete` without checking the result object. Subagents returning `{"error": ...}` displayed green `completed` in terminal traces, misleading operators into thinking tasks succeeded.
* **The Prohibition:** Never emit completion events without validating the payload structure.
* **The Enforced Rule:** Inspect the result JSON:
  ```rust
  let is_error = value.is_object() && value.get("error").is_some();
  emit(AgentLoopEvent::ToolComplete { success: !is_error, ... });
  ```
  Failures must truthfully display `FAILED` in coral red (`#e06c75`).
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs).

---

### 20. NEVER Let Burst LLM Tool Hallucinations Crash the Engine
* **Origin:** Session 22 (2026-09-11) / Vertex AI `MALFORMED_FUNCTION_CALL`.
* **The Failure Mode:** Under burst parallel dispatch, Gemini 2.5 Flash intermittently emitted raw Python code (e.g. `print(default_api.web_search(query="..."))`) instead of structured JSON objects. The API rejected this with `MALFORMED_FUNCTION_CALL`, aborting subagents within 1.1s.
* **The Prohibition:** Never allow syntactically malformed model tool calls to immediately crash the loop without recovery.
* **The Enforced Rule:** Implement a **Dual-Layer Recovery Parser**:
  1. *Prompt Guard:* Inject negative prompt directives barring Python function wrappers.
  2. *AST / Kwargs Parser:* In [`src/client/vertexai.rs`](file:///home/istari/projects/perry/src/client/vertexai.rs), intercept `finishMessage`, extract the function name and kwargs string, and reconstruct a valid JSON function call object before retrying.
* **Verification / Code Reference:** [`src/client/vertexai.rs`](file:///home/istari/projects/perry/src/client/vertexai.rs); tested in `test_recover_malformed_function_call`.

---

### 21. NEVER Retry Empty LLM Responses with Byte-for-Byte Identical Inputs
* **Origin:** Session 24 (2026-09-12) / Cache-Busting Retry Nudges.
* **The Failure Mode:** When Gemini returned an empty turn (`finishReason: "STOP"`), the engine retried with an identical request payload. Because the input was byte-for-byte identical, the provider hit its server-side prompt cache and deterministically returned empty responses in ~300ms on every retry, exhausting the retry budget.
* **The Prohibition:** Never retry transient empty turns with byte-for-byte identical input payloads.
* **The Enforced Rule:** Dynamically inject an explicit instruction into the retry input (`[Instruction: The previous tool completed successfully. Please confirm completion to the user...]`) inside the last tool result or text. This busts the provider's server-side cache hash and prompts the model to complete the turn.
* **Verification / Code Reference:** [`src/config/input.rs`](file:///home/istari/projects/perry/src/config/input.rs#L159-L203) and [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs#L3025-L3105).

---

### 22. NEVER Crash an Agent Run on Empty Output if Prior Tools Succeeded
* **Origin:** Session 24 (2026-09-12) / Graceful Synthesis Fallback.
* **The Failure Mode:** If an LLM returns empty text after tools have already executed successfully (e.g. `fs_create` created a file), bailing with `"LLM returned an empty response"` flags the entire subagent as failed, forcing orchestrators into unnecessary failure recovery.
* **The Prohibition:** Never fail an agent turn if mutations have already successfully landed.
* **The Enforced Rule:** If retries are exhausted and `has_prior_tools` is true, synthesize a graceful completion message (`"Tool execution completed successfully (<tool_names>)."`) and return `Ok((output, vec![]))` instead of bailing.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs); verified in Demo 20 and Demo 21.

---

## Era 4: Observability, Structured Plans, Taint Lifecycle & Autonomy Ladder (Sessions 25–28)

### 23. NEVER Await Subprocess Stdout Sequentially Before Draining Stderr
* **Origin:** Session 25 (2026-09-14) / OS Pipe Buffer Deadlock.
* **The Failure Mode:** If a child process writes more than the operating system pipe buffer capacity (~64KB on Linux) to `stderr` while the parent process is synchronously awaiting `stdout`, the child blocks waiting for `stderr` buffer space, while the parent blocks waiting for `stdout` EOF. The system permanently deadlocks.
* **The Prohibition:** Never read subprocess standard output and standard error sequentially.
* **The Enforced Rule:** In [`src/function.rs`](file:///home/istari/projects/perry/src/function.rs) (`run_llm_function`), child `stdout` and `stderr` must be drained concurrently using asynchronous tasks (e.g. `tokio::io::copy` or `tokio::join!`).
* **Verification / Code Reference:** [`src/function.rs`](file:///home/istari/projects/perry/src/function.rs) and [`src/agent_loop/dialog_trace.rs`](file:///home/istari/projects/perry/src/agent_loop/dialog_trace.rs).

---

### 24. NEVER Let Ahead-of-Time Plan Risk Passes Pre-Clear Act-Time Execution
* **Origin:** Session 26 (2026-09-15) / Backlog #15 (`Structured Plan & Risk Pre-Pass`).
* **The Failure Mode:** Allowing a plan pre-pass (`plan_risk_prepass`) that evaluates scheduled steps to "green-light" them opens a major vulnerability: an agent can plan a benign action, have it pre-cleared, and then supply malicious arguments at actuation time.
* **The Prohibition:** Never use plan-time evaluations to bypass act-time safety gates.
* **The Enforced Rule:** The plan pre-pass is strictly an **early red light, never a green light**. It can populate the `RiskCache` with raised floors, but **act-time evaluation remains non-negotiable**. Every action must pass through Gate 1, Gate 2, and Gate 3 at invocation time.
* **Verification / Code Reference:** [`src/agent_loop/plan.rs`](file:///home/istari/projects/perry/src/agent_loop/plan.rs).

---

### 25. NEVER Disclose Skill Catalogs to Nano Utility Workers
* **Origin:** Session 26 (2026-09-15) / Backlog #17 (`spec-17-skills.md`).
* **The Failure Mode:** Nano agents (`nano: true`) are spawned for fast, single-turn leaf operations (e.g. grep, regex extraction). Injecting full `### Available Skills` catalogs bloats their system prompts, burning unnecessary tokens and causing tool confusion.
* **The Prohibition:** Never expose skills or the `read_skill` tool to nano agents.
* **The Enforced Rule:** In [`src/config/agent.rs`](file:///home/istari/projects/perry/src/config/agent.rs), `skills_setting()` evaluates to `SkillSetting::Disabled` whenever `is_nano()` is true.
* **Verification / Code Reference:** [`src/config/agent.rs`](file:///home/istari/projects/perry/src/config/agent.rs); tested in `test_skills_setting_nano_disabled`.

---

### 26. NEVER Allow Workspace Runbook Taint to Persist Across the Entire Session
* **Origin:** Session 26 (2026-09-15) / Backlog #17 (`spec-17-skills.md`).
* **The Failure Mode:** Permanently tainting a session because a workspace skill was read in step 1 subjects all subsequent, unrelated built-in tool calls to heightened scrutiny, causing false positives and unnecessary human blocks.
* **The Prohibition:** Never use session-wide persistent taint flags for ephemeral runbooks.
* **The Enforced Rule:** Taint is **step-bound** via [`ActiveSkillTracker`](file:///home/istari/projects/perry/src/skill.rs). When a workspace skill is read, taint binds strictly to the active plan `step_id`. Calling `complete_step(step_id)` immediately unbinds the skill and clears the untrusted taint.
* **Verification / Code Reference:** [`src/skill.rs`](file:///home/istari/projects/perry/src/skill.rs); tested in `test_taint_cleared_on_complete_step`.

---

### 27. NEVER Expose Double-Prompt Gates to the Human Operator
* **Origin:** Session 28 (2026-09-17) / Backlog #19 (`Autonomy Ladder`).
* **The Failure Mode:** In standard multi-gate designs, tripping Gate 2 (`authority_exceeded`) prompted the operator for approval; after approval, Gate 3 (`%assess-risk%`) evaluated the action and prompted the operator a second time with risk analysis.
* **The Prohibition:** Never prompt the user sequentially across multiple gates for a single tool call.
* **The Enforced Rule:** Enforce the **Evaluator-First Unified Human Consultation Funnel**: Gate 3 runs *first* whenever Gate 2 trips. A single unified banner is presented to the operator displaying the authority delta, tool arguments, and the evaluator's risk rationale.
* **Verification / Code Reference:** [`src/safety.rs`](file:///home/istari/projects/perry/src/safety.rs); verified in Demo 20.

---

### 28. NEVER Pass `AICHAT_AUTONOMY` Downward to Subagents
* **Origin:** Session 28 (2026-09-17) / Backlog #19 (`Autonomy Ladder`).
* **The Failure Mode:** Passing `--autonomy` environment variables to child processes causes child agents to re-derive their own authority postures, potentially overriding explicit limits provisioned by the parent.
* **The Prohibition:** Never export macro autonomy flags into child subprocess environments.
* **The Enforced Rule:** `--autonomy` is strictly an operator macro for the root orchestrator. Subagents must be provisioned strictly via canonical `DelegatedPermissions` (`AICHAT_CAPABILITY_MASK` and `AICHAT_AUTHORITY_CEILING`).
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs).

---

## Era 5: Rebranding, Host Invariance, Production Posture & Decoupling (Sessions 29–31)

### 29. NEVER Mutate Active Repositories During Major Migrations
* **Origin:** Session 29 (2026-09-18) / Repository Anchoring Protocol.
* **The Failure Mode:** In-place renaming and deleting git remotes while actively developing risks repository corruption, dangling references, and accidental loss of unpushed commits.
* **The Prohibition:** Never perform destructive in-place migrations on live agentic repositories.
* **The Enforced Rule:** Adhere to the **Cloud-Clone-First Protocol**:
  1. Push complete 1:1 cloud replicas (all branches, commits, and tags) to the new origin.
  2. Verify cloud integrity.
  3. Clone fresh, pristine workspaces (`/projects/perry`, `/projects/innators`).
  4. Leave legacy directories intact as immutable local reference backups.
* **Verification / Code Reference:** Session 29 Summary (`docs/session-summary-2026-09-18-session29.md`).

---

### 30. NEVER Duplicate Host Discovery Logic Inside Individual Actuator Tools
* **Origin:** Session 30 (2026-09-21) / Decoupled Baseline Identity.
* **The Failure Mode:** Having `host_service`, `host_net`, and `host_resource` each run commands to discover OS, CPU model, and memory limits duplicated parsing logic, bloated JSON outputs, and interpreted metrics in a vacuum.
* **The Prohibition:** Never embed static host discovery inside telemetry actuators.
* **The Enforced Rule:** Extract all static host identification into a single dedicated authority (`tools/host_env.sh` in [`innators`](file:///home/istari/projects/innators)). Telemetry actuators must focus strictly on dynamic signals and anchor them to this baseline.
* **Verification / Code Reference:** `tools/host_env.sh` in [`innators`](file:///home/istari/projects/innators); verified in Demo 25.

---

### 31. NEVER Rely on `uptime -p` or Non-Portable GNU Flags in Actuators
* **Origin:** Session 30 (2026-09-21) / 10–15+ Year Portability Invariants.
* **The Failure Mode:** Calling `uptime -p` causes immediate crashes on minimal BusyBox and Alpine Linux environments because BusyBox `uptime` does not support `-p`.
* **The Prohibition:** Never use modern GNU coreutils extensions in baseline SRE tools.
* **The Enforced Rule:** Calculate uptime from pure integer seconds in `/proc/uptime`:
  ```bash
  uptime_s=$(cut -d. -f1 /proc/uptime)
  echo "$((uptime_s / 3600))h $(( (uptime_s % 3600) / 60 ))m"
  ```
  Guarantees zero-crash portability across modern Linux, 10-year-old CentOS, Alpine, and OpenWrt.
* **Verification / Code Reference:** `tools/host_env.sh` in [`innators`](file:///home/istari/projects/innators).

---

### 32. NEVER Assign Safety Metadata Monolithically at the Script Level
* **Origin:** Session 27 & 30 (2026-09-16/21) / `extract_shell_function`.
* **The Failure Mode:** Labeling a multi-tool script (`tools.sh`) as `disruptive` blocks read-only subcommands (`list_items`) under a `safe` ceiling; labeling it `safe` allows destructive subcommands (`clear_all`) to bypass gates.
* **The Prohibition:** Never evaluate safety metadata on a monolithic multi-tool script.
* **The Enforced Rule:** Annotate metadata per `# @cmd` subcommand. At evaluation time, `aichat` must slice out *only* the invoked function's source code and doc-comments via `extract_shell_function`, isolating it from sibling functions.
* **Verification / Code Reference:** [`src/agent_loop.rs`](file:///home/istari/projects/perry/src/agent_loop.rs); verified in Demo 3.

---

### 33. NEVER Rely on Legacy Configuration Paths Without Subprocess Injection
* **Origin:** Session 31 (2026-09-22) / Dedicated Configuration Decoupling.
* **The Failure Mode:** When migrating from `~/.config/aichat/` to `~/.config/perry/`, existing child tools or subagents that execute `aichat` fall back to missing or unconfigured defaults.
* **The Prohibition:** Never decouple configuration directories without ensuring backward compatibility in child processes.
* **The Enforced Rule:** In [`src/function.rs`](file:///home/istari/projects/perry/src/function.rs) (`eval_shell`), automatically inject both `PERRY_CONFIG_DIR` and `AICHAT_CONFIG_DIR` pointing to the resolved Perry configuration root.
* **Verification / Code Reference:** [`src/function.rs`](file:///home/istari/projects/perry/src/function.rs).

---

### 34. NEVER Make Actuator Environment Variables Mandatory Without Fallbacks
* **Origin:** Session 31 (2026-09-22) / Hierarchical Resolution.
* **The Failure Mode:** Declaring required `argc` environment flags (e.g. `# @env WEB_SEARCH_MODEL!`) aborts tool execution during pre-flight before `main()` can run if the operator did not manually `export` the variable.
* **The Prohibition:** Never use required `argc` environment flags (`!`) in actuator scripts.
* **The Enforced Rule:** Actuator scripts must make environment variables optional and follow a strict hierarchical fallback:
  $$\text{Explicit Env Var} \longrightarrow \text{Tool Config Key} \longrightarrow \text{General Model Config} \longrightarrow \text{Stable Default}$$
  Inject resolved variables into tool environments in `eval_shell`.
* **Verification / Code Reference:** `tools/web_search_perry.sh` and [`src/function.rs`](file:///home/istari/projects/perry/src/function.rs).

---

### 35. NEVER Dump Diagnostic Scripts as Executable Binaries in Shared Dirs
* **Origin:** Session 31 (2026-09-22) / Telemetry Artifact Defanging.
* **The Failure Mode:** Dumping generated diagnostic scripts with `.sh` extensions and executable permissions into `/tmp` creates a severe local execution hazard for operators or cron jobs.
* **The Prohibition:** Never write unverified diagnostic shell scripts as executable artifacts.
* **The Enforced Rule:** In `%distill-telemetry%`, always defang dumped scripts:
  * Force extension to `.txt` (e.g. `/tmp/triage-script-<pid>.txt`).
  * Explicitly apply `chmod 600`.
  * Return clickable `file://` hyperlinks for inspection rather than direct execution pointers.
* **Verification / Code Reference:** `assets/roles/%distill-telemetry%.md` and Session 31 Summary.

---

### 36. NEVER Run Environment-Sensitive Tests in Parallel
* **Origin:** `lesssons-learned.md` (2026-09-21) / Concurrency Deadlock Bug.
* **The Failure Mode:** Over 560 unit tests validate authority ceilings, capability masks, and escalation depths by mutating process-wide environment variables guarded by `MASK_ENV_LOCK`. Running tests in parallel causes threads reading environment variables outside the lock to race or deadlock indefinitely.
* **The Prohibition:** Never run the test suite with unrestricted thread parallelism when validating safety gates.
* **The Enforced Rule:** Always execute safety-sensitive test runs serially:
  ```bash
  cargo test -- --test-threads=1
  ```
* **Verification / Code Reference:** [`lesssons-learned.md`](file:///home/istari/projects/perry/lesssons-learned.md#L239-L250).

---

### 37. NEVER Use Functional Loops in Tight Nushell Worker Scripts
* **Origin:** Repository Rules & Nushell Optimization Mandate.
* **The Failure Mode:** Using nested `.each`, `.filter`, or `.any` closures inside tight loops allocates VM stack frames and environment captures on every item, significantly degrading execution speed.
* **The Prohibition:** Never use functional closure loops inside tight Nushell data processing loops.
* **The Enforced Rule:** Replace functional loops with procedural keywords (`for`, `match`), use structural pattern matching instead of optional member paths (`?.`), combine sequential record modifications into single `merge` calls, and guard string splits with cheap `str contains` checks.
* **Verification / Code Reference:** [`scripts/run-demos.nu`](file:///home/istari/projects/perry/scripts/run-demos.nu) and `AGENTS.md`.

---

## The Top 10 Non-Negotiable Invariants

For quick reference during architectural reviews and pair programming, verify your changes against these 10 invariants:

```text
 1. [The LLM Is Not a Pardoner]
    Evaluators only tighten via stricter_of(); models cannot loosen, pardon, or bypass deterministic rules.

 2. [Immutable Subprocess Sandboxes]
    No downward permit tokens (permit-<uuid>) in-flight; subagent masks and ceilings are frozen at spawn.

 3. [Option B Opportunistic Remediation]
    Reversible mutations take pre-flight file backups to $XDG_RUNTIME_DIR to step down authority autonomously.

 4. [Parallel History Preservation]
    Executed tool results are never filtered or dropped; history dropping causes infinite prompt loops.

 5. [Acyclic Tool Pipe Routing]
    Piped tool chains walk a visited HashSet; detected cycles abort immediately.

 6. [16KB Stream Auto-Capping]
    Tool outputs >16KB are spooled to disk, returning a preview and path pointer to protect LLM context.

 7. [Evaluator-First Unified Funnel]
    Gate 3 (%assess-risk%) runs before Gate 2 prompts, presenting a single combined banner without double-prompts.

 8. [Step-Bound Skill Taint]
    Workspace skill taint binds to active plan steps and clears immediately upon step completion.

 9. [10–15+ Year Host Portability]
    No GNU-only flags (no uptime -p); uptime uses integer /proc/uptime to guarantee Alpine/BusyBox support.

10. [Serial Test Execution for Environment Mutexes]
    Safety gate tests run with cargo test -- --test-threads=1 to prevent deadlock on MASK_ENV_LOCK.
```
