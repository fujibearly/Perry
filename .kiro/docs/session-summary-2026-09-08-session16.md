# Session Summary 16: Prohibition of Downward Permit Propagation & Hard Child Authority Ceilings

**Date:** 2026-09-09  
**Branch:** `feat/tool-safety-permission-boundary`  
**Test Suite Status:** 485 tests passing baseline; refactoring in progress for FR-6d.24.  
**Specification:** `.kiro/specs/tool-safety-modes/requirements.md` (`FR-6d.24`), `tasks.md` (`Task 6d.31`, `Task 6d.32`)

---

## 1. Executive Summary

In Session 16, we addressed an architectural contradiction introduced in Session 14's `FR-6d.22` (Downward Supervisory Verdict & Permit Propagation).

During review of Demo 3 traces, it was observed that child agent `coder` (spawned with ceiling `reversible`) invoked tool `fs_create` (classified as `disruptive`). Under the prior `FR-6d.22` design, the child escalated to its supervisor over mTLS, the supervisor's `%assess-risk%` evaluated the call, and the supervisor returned a `Continue` verdict containing an `ExecutionPermit` token (`permit-<uuid>`). The child accepted this permit token, bypassed its own authority ceiling check (`supervisory_approved = true`), and actuated the disruptive tool.

This behavior violated two core architectural tenets:
1. **The LLM is NOT a Pardoner:** Risk assessment by an LLM is strictly an extra safety filter designed to stop dangerous actions. It must never act as a pass-through approval mechanism or permission elevation grant to relax existing policies, authority ceilings, or capability masks.
2. **Zero Downward Permit Propagation:** An agent's execution boundary is defined statically upfront by its `DelegatedPermissions` (`mask` and `ceiling`). An agent cannot elevate its permissions in-flight, nor can an orchestrator issue downward permits to allow a sub-agent to breach its sandbox.

Under **`FR-6d.24`**, we unify Gate 1 (`capability_denied`) and Gate 2 (`authority_exceeded`) into an immutable, hard process sandbox boundary for child agents:
- Sub-agents cannot escalate over mTLS to elevate either their capability mask or their authority ceiling in-flight.
- When an action exceeds the sub-agent's ceiling, actuation is blocked immediately (`authority_exceeded`).
- The sub-agent unwinds any pre-mutation journal entries recorded in that execution session, halts, and returns structured `status: "permission_blocked", reason: "authority_exceeded"` to the parent orchestrator.
- The parent orchestrator ingests the structured block into context and can re-delegate with the required ceiling upfront (subject to the 2-attempt circuit breaker) or execute the action directly.

---

## 2. Key Architectural Invariants

### 2.1 Unified Sandboxing Principle
An agent's execution boundary consists of two static, immutable dimensions provisioned upfront in `DelegatedPermissions`:
1. `mask`: `readonly` vs `mutating` (Gate 1)
2. `ceiling`: `safe` $\rightarrow$ `catastrophic` (Gate 2)

Neither dimension can be elevated in-flight over mTLS. A child agent process is sandboxed strictly within these bounds for its entire lifetime.

### 2.2 Unwind & Bounded Re-Delegation (Fail-Closed)
When a child process trips either Gate 1 (`capability_denied`) or Gate 2 (`authority_exceeded`):
1. Actuation is immediately halted.
2. Pre-mutation journal entries are rolled back via `journal.replay_last()`.
3. The child emits a structured failure:
   ```json
   {
     "status": "permission_blocked",
     "reason": "authority_exceeded",
     "tool": "fs_create",
     "required_permission": {
       "mask": "mutating",
       "ceiling": "disruptive"
     },
     "rollback_executed": true
   }
   ```
4. The parent orchestrator receives this as a tool result and determines whether to re-delegate with elevated permissions (if within the parent's ceiling) or execute directly.
5. Unbounded loops are prevented by the 2-attempt re-delegation circuit breaker (`redelegate_attempts`).

### 2.3 Gate 3 (`%assess-risk%`) is Tightening-Only
Gate 3 runs only when an action is already within the agent's deterministic mask and ceiling. It evaluates concrete execution facts (command, args, script source) and clamps strictly via `stricter_of(base_tier, verdict.tier)`. It can only *tighten* restrictions:
- It can raise the required tier (`reversible` $\rightarrow$ `disruptive`).
- It can require human terminal approval or halt on low confidence / security concerns.
- It can NEVER lower an authority requirement or pardon an authority ceiling breach.

---

## 3. Implementation Tasks Planned

1. **`src/safety.rs`:**
   - Remove `token: Option<String>` and `risk_verdict: Option<RiskVerdict>` from `VerdictMsg`.
   - Update tests that construct or assert `VerdictMsg.token`.
2. **`src/agent_loop.rs`:**
   - In `eval_single_tool`:
     - Remove `supervisory_approved`.
     - Remove in-flight escalation of `authority_exceeded` for child processes (`parent_info.is_some()`). Return `Ok(denied)` immediately.
     - For root orchestrator (`parent_info.is_none()`), retain human terminal prompt (`prompt_human_verdict`).
     - In Gate 3, remove `if !supervisory_approved` bypass. Gate 3 is mandatory for all non-safe actions.
   - In child loop:
     - Expand `permission_blocked` handler to catch BOTH `capability_denied` and `authority_exceeded`.
     - Unwind journal, emit event, and return structured `permission_blocked` payload with required ceiling.
   - In `eval_agent_tool_subprocess`:
     - Handle `permission_blocked` with `authority_exceeded` and generate clear guidance for orchestrator re-delegation.
   - Remove `supervisory_decision_*` permit-token logic from escalation handler.
3. **`scripts/run-demos.nu`:**
   - Update Demo 20 to test hard authority ceiling block and orchestrator re-delegation.
   - Verify Demo 3 passes with clean re-delegation and zero downward permits.
## 4. Verification & Results

- **Unit & Integration Tests:**
  - 478 unit tests + 5 catalog override tests + 3 web asset security tests = **486 passed, 0 failed**.
- **Static Analysis:**
  - `cargo clippy --all-targets -- -D warnings`: passed with 0 warnings.
- **Binary Build:**
  - `cargo build --release`: clean build.
- **Live Demo Verification:**
  - **Demo 16 (Deterministic Offline Escalation & Journal):** 100% passed (fail-closed timeout, 0600 permissions, rollback replay).
  - **Demo 20 (Hard Authority Ceiling Sandboxing & Re-Delegation):** 100% passed (coder blocked by reversible ceiling with zero downward permits, parent re-delegated with disruptive ceiling).
  - **Demo 3 (Planning Tool & Sub-agent Delegation):** 100% passed (coder blocked on disruptive `fs_create`, orchestrator updated plan and re-delegated with disruptive ceiling, task completed cleanly).
- **Git Commit:**
  - Committed in `de0f501`: `feat: prohibit downward permit propagation and enforce hard child authority ceilings (FR-6d.24)`.

