# Session Summary & Agent Handoff (Session 11: 2026-09-08)

**Period:** `2026-09-08` (Permission vs. Authorization Architecture Redesign, FR-6d.18–FR-6d.21, Tasks 6d.20–6d.26).  
**Primary Engine:** `/home/istari/projects/aichat`  
**Branch:** `feat/tool-safety-permission-boundary` (branched from `feat/tool-safety-6d` at `13c6361`)  
**Commit:** [`e64d146`](file:///home/istari/projects/aichat) (`feat(safety): implement permission vs authorization boundary and bounded re-delegation (FR-6d.18-21)`)  
**Companion Repo:** `/home/istari/projects/llm-functions`  
**Companion Branch:** `feat/tool-safety-permission-boundary`  
**Companion Commit:** [`cfb2f7e`](file:///home/istari/projects/llm-functions) (`feat(orchestrator): expose permissions contract in functions schema`)  
**Version:** `v0.31.0-fork.9`  

---

## 1. Executive Summary

This session executed a major architectural redesign establishing a strict, engine-enforced boundary between **Permissions** (technical process execution capabilities provisioned at spawn time) and **Authorization** (supervisory goal-alignment and advisory risk evaluation via the Session-10 Should Gate).

### Core Problem Solved
Previously, a sub-agent spawned under a `readonly` capability mask could attempt a mutating tool, escalate over mTLS to its supervisor, and receive a `Continue` verdict. This constituted an in-flight "permission loan" that eroded process sandboxing: the orchestrator approved the mutation because the orchestrator itself had mutating capabilities and lent them to the child.

### Redesigned Architecture
1. **Hard Process Capability Boundary (No In-Flight Elevation):**
   - The `#6a` capability mask is an engine-enforced, static sandbox boundary.
   - A `readonly` process cannot have its mask elevated in-flight over mTLS by any verdict.
   - Removed mTLS escalation from `capability_denied_result` in [`src/agent_loop.rs`](file:///home/istari/projects/aichat/src/agent_loop.rs).
2. **Hierarchical Upfront Provisioning ([`DelegatedPermissions`](file:///home/istari/projects/aichat/src/function.rs)):**
   - The orchestrator provisions sub-agents with execution capabilities at delegation time via a strongly typed contract (`permissions: { mask, ceiling }`, with flat argument fallbacks `permissions_mask`, `permissions_ceiling`).
   - The engine validates `requested <= parent`. Unknown or malformed inputs clamp closed to the safe floor (`readonly`, `safe`). `Catastrophic` authority cannot be delegated autonomously (reserved to humans).
3. **Sub-Agent Unwind & Clean Exit on Capability Block:**
   - When a sub-agent trips `capability_denied`, it halts actuation immediately, unwinds durable rollback journal entries (`journal.replay_last()`), emits `AgentLoopEvent::CapabilityBlocked`, and exits cleanly returning structured JSON (`status: "permission_blocked"`).
4. **Orchestrator Ingestion & Bounded Re-Delegation:**
   - The orchestrator ingests `permission_blocked` as a tool result with contextual guidance.
   - The orchestrator reasons from its full context (user prompt + triage summary) whether to re-delegate with explicit mutating permissions or prompt the user.
   - A per-`(agent, task)` circuit breaker (cap of 2 attempts) prevents infinite retry loops.
5. **Preservation of the Session-10 Should Gate:**
   - The authority-ceiling escalation path (`authority_exceeded`) over mTLS remains intact for higher blast-radius operations, retaining the full supervisory Should Gate (Protected Policy check, anti-spoofed static tier floor, reversibility verification, `%assess-risk%`, stricter-only clamping, and fail-toward-`Human`).
   - Added defense-in-depth rejection in [`handle_escalation_request`](file:///home/istari/projects/aichat/src/agent_loop.rs) for any incoming `capability_denied` escalation.

---

## 2. Key Code Changes

### [`src/function.rs`](file:///home/istari/projects/aichat/src/function.rs)
- Added [`DelegatedPermissions`](file:///home/istari/projects/aichat/src/function.rs) struct with:
  - `parse_from_value`: extracts permissions from nested objects or flat fallbacks (`permissions_mask`, `permissions_ceiling`), parsing both JSON objects and JSON strings.
  - `validate_against_parent`: validates that `requested.mask` cannot be mutating if parent is readonly, `requested.ceiling <= parent_ceiling`, and catastrophic ceiling is rejected.
  - `resolve_for_call`: convenience entrypoint defaulting to the safe floor (`readonly`, `safe`).
- Added [`FunctionDeclaration::enrich_agent_permissions_schema`](file:///home/istari/projects/aichat/src/function.rs), enriching agent declarations dynamically so LLM providers see the contract parameters.
- Added comprehensive unit tests for nested/flat parsing, validation, parent clamping, and schema enrichment.

### [`src/agent_loop.rs`](file:///home/istari/projects/aichat/src/agent_loop.rs)
- Added `AgentLoopEvent::CapabilityBlocked { name, unwound }` and formatted in trace rendering.
- In [`eval_single_tool`](file:///home/istari/projects/aichat/src/agent_loop.rs): removed mTLS escalation from `capability_denied_result`, returning `Ok(denied)` immediately to the child loop. Preserved mTLS escalation for `authority_exceeded` and `risk_blocked`.
- In [`eval_agent_tool_subprocess`](file:///home/istari/projects/aichat/src/agent_loop.rs):
  - Resolves permissions via `DelegatedPermissions::resolve_for_call`.
  - Provisions `AICHAT_CAPABILITY_MASK` and `AICHAT_AUTHORITY_CEILING` environment variables on the child process.
  - Detects child `permission_blocked` JSON output (even if embedded in stdout) and constructs guidance for the orchestrator.
- In `AgentLoop::run`:
  - Added child unwinding: if `under_readonly_mask()` is true and a tool was blocked by `capability_denied`, replays the durable journal, emits trace, prints JSON payload, and returns `AgentLoopOutput`.
  - Added re-delegation circuit breaker: tracks `redelegation_counts` per `(agent, task)` and trips after 2 attempts.
- In [`handle_escalation_request`](file:///home/istari/projects/aichat/src/agent_loop.rs): added defense-in-depth rejection for `esc.reason == "capability_denied"`, returning `VerdictDecision::Halt`.

### [`agents/orchestrator/functions.json`](file:///home/istari/projects/llm-functions/agents/orchestrator/functions.json) (`llm-functions`)
- Exposed `permissions`, `permissions_mask`, and `permissions_ceiling` properties on `researcher` and `coder` agent tool definitions.

### [`scripts/run-demos.nu`](file:///home/istari/projects/aichat/scripts/run-demos.nu)
- Updated **Demo 20** to provision `permissions_mask: "mutating"` and `permissions_ceiling: "reversible"`, verifying the authority-ceiling escalation flow through the Should Gate.
- Added **Demo 21**, verifying the full capability-mask block $\rightarrow$ journal unwind $\rightarrow$ `permission_blocked` report $\rightarrow$ orchestrator re-delegation with mutating permissions $\rightarrow$ successful actuation.
- Updated `aichat_bin` resolution to pick the newer binary between debug and release, and support an `AICHAT_BIN` environment override.

---

## 3. Human-in-the-Loop & User Input Considerations

In light of user interaction constraints ("beware of the possible need for user input"):

1. **Autonomous Re-Delegation vs. Human Clarification:**
   - When an orchestrator receives a `permission_blocked` tool result, it has the authority to provision mutating permissions *only if* the mutation is authorized and within its own ceiling.
   - If the user's intent is ambiguous or the orchestrator was not instructed to mutate, the orchestrator LLM is expected to ask the user for confirmation/clarification rather than blindly re-delegating with elevated permissions.
2. **Interactive Terminal vs. Headless Execution:**
   - In interactive mode (`*IS_STDOUT_TERMINAL`), if an action or escalation exceeds the supervisor's ceiling, the supervisor invokes the single-key HITL CLI prompt (`prompt_human_verdict`).
   - In headless / non-terminal execution (such as scripted subshells or background tasks), `prompt_human_verdict` fails closed to `VerdictDecision::Halt` to prevent hanging indefinitely on stdin.
3. **Piped Invocations for Tools with Confirmation Prompts:**
   - When tools like `fs_write` prompt confirmation on stdin, non-interactive execution must provide input via pipe (e.g. `"" | with-env ...` or `echo y | aichat ...`) to prevent deadlocks in headless runners.

---

## 4. Verification & Quality Gates

| Check | Result | Details |
|---|---|---|
| `cargo test --bin aichat` | **PASS** | 463 unit tests passed (0 failed). |
| `cargo clippy --bin aichat -- -D warnings` | **PASS** | 0 warnings across the binary crate. |
| `cargo build --bin aichat` | **PASS** | Clean build for debug binary. |
| `scripts/run-demos.nu` | **PASS** | All 21 demos completed with exit code 0. |
| Demo 20 (Should Gate) | **PASS** | Coder escalated `authority_exceeded` $\rightarrow$ supervisor Should Gate evaluated risk $\rightarrow$ `Continue` $\rightarrow$ actuated. |
| Demo 21 (Capability Block) | **PASS** | Coder blocked `capability_denied` $\rightarrow$ unwound journal $\rightarrow$ `permission_blocked` $\rightarrow$ orchestrator re-delegated with mutating permissions $\rightarrow$ actuated. |

---

## 5. Specification & Documentation Traceability

- [`.kiro/specs/tool-safety-modes/requirements.md`](file:///home/istari/projects/aichat/.kiro/specs/tool-safety-modes/requirements.md):
  - `FR-6d.18`: Hierarchical Delegation Permissions Contract (`DelegatedPermissions`)
  - `FR-6d.19`: Hard Capability-Mask Block & Deterministic Unwinding
  - `FR-6d.20`: Orchestrator Loop Ingestion & Bounded Re-Delegation
  - `FR-6d.21`: Escalation Handler Defense-in-Depth
- [`.kiro/specs/tool-safety-modes/design.md`](file:///home/istari/projects/aichat/.kiro/specs/tool-safety-modes/design.md):
  - Added As-Built Notes for Permission vs. Authorization Architecture Redesign (Refinement to Decision B).
- [`.kiro/specs/tool-safety-modes/tasks.md`](file:///home/istari/projects/aichat/.kiro/specs/tool-safety-modes/tasks.md):
  - Tasks `6d.20` through `6d.26` all completed and checked off.
- [`.kiro/docs/roadmap.md`](file:///home/istari/projects/aichat/.kiro/docs/roadmap.md):
  - Status Table row `6d` updated to `✓ Done (Permission vs. Authorization redesign)`.

---

## 6. Git Status

- **`aichat`:** On branch `feat/tool-safety-permission-boundary`, committed at [`e64d146`](file:///home/istari/projects/aichat). Clean working tree.
- **`llm-functions`:** On branch `feat/tool-safety-permission-boundary`, committed at [`cfb2f7e`](file:///home/istari/projects/llm-functions). Clean working tree.
- **Constraint:** Strictly local commits; no remote push.
