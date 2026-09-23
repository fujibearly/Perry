# Specification: Backlog #15 — Plan-Driven Structured Execution & Whole-Plan Risk Pre-Pass

**Document:** `spec-15-structured-plan.md`  
**Parent Epic:** `.kiro/specs/structured-plan-and-skills/`  
**Target Branch:** `feat/structured-plan`  
**Status:** 🔜 Proposed (Base Spec A — Hardened)  
**Effort:** Medium (~250–350 lines in `src/agent_loop/plan.rs` and `src/agent_loop.rs`)  
**Base Floor:** #6 Safety Umbrella (`#6a` capability mask, `#6b` blast radius, `#6c` `%assess-risk%`, `#6d` mTLS).

---

## 1. Motivation & Problem Statement

Today, the `_plan` pseudo-tool (Catalog item #11, [`src/agent_loop.rs:2590`](file:///home/istari/projects/perry/src/agent_loop.rs#L2590)) is a **free-text scratchpad**. The model writes an arbitrary text thought, which the engine logs, returns `"acknowledged"`, and injects into the next turn's dialog history.

This design suffers from three structural deficiencies:
1. **No Execution Coupling:** The engine has no semantic visibility into whether subsequent tool calls align with the stated plan or deviate from it.
2. **No Progress Visibility:** The operator and supervisor cannot observe which step the agent is executing, which steps remain pending, or which step failed.
3. **No Cross-Step Risk Foresight:** Pre-actuation risk evaluation (#6c) evaluates each tool call in isolation at act-time. It cannot observe multi-step blast radii (e.g., Step 2 stages a file that Step 4 wipes, or a sequence of individually low-risk probes collectively exfiltrates sensitive state).

Backlog #15 transforms `_plan` into an **ordered step list** that drives execution tracking and enables an **ahead-of-time whole-plan risk pre-pass** populating the `#6c` `RiskCache`.

---

## 2. Architectural Invariants

### Invariant 1: Structured Planning is an Optimization Layer (Fix 1 & Fix 5)
> **Hard Invariant:** A structured plan is an optimization and observability layer, **never a correctness dependency**.

If an LLM provider or model ignores structured schema fields, emits a free-form string, emits `{"thought": "..."}`, emits malformed/duplicate step IDs, or omits `_plan` entirely, the agent loop **must degrade cleanly to the standard ReAct execution loop without error or behavioral regression**. Any structural parsing or validation failure falls back to a legacy scratchpad.

### Invariant 2: The LLM is Not a Pardoner (Strictly Stricter-Only Pre-Pass — Fix 2)
> **Hard Invariant:** The whole-plan risk pre-pass is an early red-light, **never a green-light**.

1. **Pre-pass is Advisory & Raise-Only:** The pre-pass evaluates model-declared preview intent (`args_preview`). Because previewed arguments are speculative and can diverge from actual act-time execution, a pre-pass assessment can **only pre-raise the floor** in the `#6c` monotonic `RiskCache`.
2. **No Approval by Proxy:** A pre-pass verdict never substitutes for act-time evaluation of the concrete invocation. `RiskCache` entries are keyed on the **actual resolved invocation** (`(tool_name, concrete_args)`), never on speculative preview strings.
3. **Act-Time Remains the Non-Negotiable Gate:** Every actuated tool call is independently evaluated against `#6a` capability masks, `#6b` blast-radius ceilings, and `#6d` escalation. The pre-pass can only cause an earlier or firmer block; it can never grant permission.

### Invariant 3: Base Branch Dependency & Standalone Degradation
> **Hard Invariant:** Spec A requires `#6c`'s `RiskCache` and `%assess-risk%` on the base branch.

If Spec A is compiled or executed on a base branch where `#6c` is absent, `plan_risk_prepass()` **cleanly no-ops**, and the engine degrades gracefully to act-time-only evaluation. Spec A's step list parsing, state progression, and terminal rendering remain fully functional and verifiable in isolation.

---

## 3. Data Structures & Schema (Fix 1, Fix 3, Fix 5)

### 3.1 Plan Payload Representation (`src/agent_loop/plan.rs`)

To guarantee Invariant 1 across all provider idiosyncrasies, `PlanPayload` implements a robust multi-tiered fallback:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: usize,
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_preview: Option<serde_json::Value>,
    /// Recorded for observability and future topological enforcement;
    /// Spec A tracks steps as an ordered list.
    #[serde(default)]
    pub depends_on: Vec<usize>,
    #[serde(default = "default_pending")]
    pub status: StepStatus,
}

fn default_pending() -> StepStatus {
    StepStatus::Pending
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredPlan {
    pub objective: String,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyThoughtObject {
    pub thought: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawPlanPayload {
    Structured(StructuredPlan),
    LegacyThought(LegacyThoughtObject),
    LegacyString(String),
    LegacyCatchAll(serde_json::Value),
}

#[derive(Debug, Clone)]
pub enum PlanPayload {
    Structured(StructuredPlan),
    Legacy(String),
}

impl PlanPayload {
    /// Deserializes JSON input with guaranteed fallback to Legacy mode.
    /// Never returns an Err; invalid or malformed shapes degrade gracefully.
    pub fn parse_flexible(raw: serde_json::Value) -> Self {
        if let Ok(raw_payload) = serde_json::from_value::<RawPlanPayload>(raw.clone()) {
            match raw_payload {
                RawPlanPayload::Structured(plan) => {
                    // Structural validation: non-empty objective, valid steps with unique IDs
                    if !plan.objective.trim().is_empty() 
                        && !plan.steps.is_empty() 
                        && Self::validate_steps(&plan.steps) 
                    {
                        PlanPayload::Structured(plan)
                    } else {
                        // Malformed structured payload degrades to legacy string (Fix 5)
                        PlanPayload::Legacy(raw.to_string())
                    }
                }
                RawPlanPayload::LegacyThought(obj) => PlanPayload::Legacy(obj.thought),
                RawPlanPayload::LegacyString(s) => PlanPayload::Legacy(s),
                RawPlanPayload::LegacyCatchAll(val) => {
                    if let Some(thought) = val.get("thought").and_then(|t| t.as_str()) {
                        PlanPayload::Legacy(thought.to_string())
                    } else {
                        PlanPayload::Legacy(val.to_string())
                    }
                }
            }
        } else {
            PlanPayload::Legacy(raw.to_string())
        }
    }

    fn validate_steps(steps: &[PlanStep]) -> bool {
        let mut seen_ids = std::collections::HashSet::new();
        for step in steps {
            if step.intent.trim().is_empty() || !seen_ids.insert(step.id) {
                return false;
            }
        }
        true
    }
}
```

### 3.2 Dynamic `_plan` Tool Declaration

The pseudo-tool declaration presented to the model in `src/agent_loop.rs` advertises structured parameters alongside `thought`:

```json
{
  "name": "_plan",
  "description": "Formulate or update a structured execution plan. You may output an ordered step list or a free-text thought.",
  "parameters": {
    "type": "object",
    "properties": {
      "objective": {
        "type": "string",
        "description": "High-level goal of the execution sequence."
      },
      "steps": {
        "type": "array",
        "description": "Ordered execution steps with declared tools and intent.",
        "items": {
          "type": "object",
          "properties": {
            "id": { "type": "integer" },
            "intent": { "type": "string" },
            "tool": { "type": "string" },
            "args_preview": { "type": "object" },
            "depends_on": {
              "type": "array",
              "items": { "type": "integer" }
            }
          },
          "required": ["id", "intent"]
        }
      },
      "thought": {
        "type": "string",
        "description": "Legacy free-text reasoning scratchpad."
      }
    }
  }
}
```

---

## 4. Execution Lifecycle & Hot-Path Changes (Fix 2, Fix 3)

```mermaid
sequenceDiagram
    autonumber
    participant Model as LLM Provider
    participant Loop as Agent Loop (run())
    participant Tracker as PlanTracker
    participant Risk as Safety Engine (#6c)
    participant Actuator as Tool Dispatcher

    Model-->>Loop: tool_call: _plan(objective, steps: [1, 2, 3])
    Loop->>Tracker: parse_flexible(payload)
    alt Valid Structured Plan
        Tracker-->>Loop: PlanTracker initialized (Ordered step list)
        Loop->>Risk: plan_risk_prepass(steps, RiskCache)
        Note over Risk: Evaluates preview intent for mutating steps;<br/>pre-raises RiskCache verdicts.
        Loop-->>Model: tool_result: "Plan acknowledged with 3 steps"
    else Legacy Object {"thought": "..."} / String / Malformed
        Tracker-->>Loop: Fallback to Legacy Mode
        Loop-->>Model: tool_result: "acknowledged"
    end

    loop Execution Turns
        Model-->>Loop: tool_call: command(concrete_args)
        Loop->>Tracker: update_active_step(...)
        Loop->>Risk: act_time_eval(command, concrete_args, RiskCache)
        Note over Risk: Evaluates concrete execution facts.<br/>Floor meets or exceeds pre-raised RiskCache.
        Risk-->>Loop: Verdict (Approved or Blocked)
        alt Approved
            Loop->>Actuator: Execute tool
            Actuator-->>Loop: Tool Output
            Loop->>Tracker: mark_step_completed(...)
        else Blocked
            Loop->>Tracker: mark_step_failed(...)
        end
    end
```

### 4.1 Scope of Plan-Time Risk Pre-Pass (`plan_risk_prepass`)
1. **Advisory Pre-Raise Only:** Scans `steps` for mutating tools. If `#6c` is active, evaluates speculative intent. If risk is assessed higher than static tier, inserts a pre-raised floor entry into `RiskCache`.
2. **Keying Contract:** When the model invokes a tool at act-time, the safety engine keys lookups on the concrete execution payload. A pre-pass record pre-conditions the cache; it **never authorizes** a divergent invocation.
3. **No-Op Base Degradation:** If `#6c` is absent, the pass returns `Ok(())` immediately.

---

## 5. Observability & Terminal Rendering

When a structured plan is updated, the engine emits `AgentLoopEvent::PlanStepUpdated`:
```text
plan: objective: "Migrate database schema and verify indexes"
plan:   [✓] 1. Backup accounts table (pg_dump)
plan:   [▶] 2. Apply alter table migration (sql)
plan:   [ ] 3. Verify index status (sql)
```

If an ordered step trips the pre-pass risk cache:
```text
plan: step 2 flagged by pre-pass: disruptive (table lock expected) [cached floor]
```

---

## 6. Verification Plan (Fix 4, Fix 5)

### 6.1 In-Module Unit Tests (`src/agent_loop/plan.rs`, `cargo test --lib agent_loop`)

1. **Flexible Deserialization & Legacy Degradation (`test_plan_payload_deserialization_and_degradation`):**
   - Assert structured JSON with valid fields produces `PlanPayload::Structured`.
   - Assert `{"thought": "examine logs"}` produces `PlanPayload::Legacy("examine logs")`.
   - Assert raw string `"examine logs"` produces `PlanPayload::Legacy("examine logs")`.
   - Assert unexpected object shapes produce `PlanPayload::Legacy(...)`.
2. **Malformed Structural Degradation (`test_malformed_plan_degradation`):**
   - Feed plan with duplicate step IDs, non-positive IDs, and empty objectives.
   - Assert parser returns `PlanPayload::Legacy` without errors or panic.
3. **Ordered Step List State Transitions (`test_step_list_progress`):**
   - Initialize `PlanTracker` with a 3-step plan.
   - Transition Step 1 from `Pending` $\rightarrow$ `InProgress` $\rightarrow$ `Completed`.
   - Assert internal tracker state and step queries reflect transitions accurately.
4. **Plan-Time Risk Pre-Pass Unit Test (`test_plan_risk_prepass_unit`):**
   - Provide a mock step list with a destructive command.
   - Execute `plan_risk_prepass()` against an in-memory `RiskCache`.
   - Assert verdict is inserted into `RiskCache`.
5. **Base Branch Degradation (`test_prepass_noop_without_6c`):**
   - Execute `plan_risk_prepass()` in an environment without `#6c` config.
   - Assert pass returns `Ok(())` with zero side effects.

### 6.2 Loop-Level Integration Scope & #11 Dependency
- Full end-to-end multi-turn execution of `run()` driving live tool dispatches and asserting step transitions depends on **Backlog #11 (Mock-Client Test Seam for Loop Coverage)**.
- For Backlog #15, loop wiring is verified via:
  - Deterministic unit tests over the pure `PlanTracker` and `plan_risk_prepass` functions.
  - Manual interactive verification: run `aichat "plan and check disk usage"` and observe `/dev/tty` step transitions.
