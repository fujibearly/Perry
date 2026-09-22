# Autonomy Ladder (`--autonomy`) — Design

## Architectural Overview

The Autonomy Ladder is an **operational posture preset** implemented as a root-level macro over the two independent axes created in Backlog #6:

```
┌─────────────────────────────────────────────────────────────┐
│                    CLI & User Intent Layer                  │
│       `--autonomy readonly`     `--autonomy consult`        │
│                    `--autonomy reversible`                  │
└──────────────────────────────┬──────────────────────────────┘
                               │ Root-Only Macro Expansion
                               ▼
┌──────────────────────────────┴──────────────────────────────┐
│                    Engine Mechanics Layer                   │
│   1. Capability Mask (Gate 1)    2. Authority Ceiling (Gate 2)│
│      [readonly | mutating]          [Safe | Reversible | …] │
└──────────────────────────────┬──────────────────────────────┘
                               │ Child Process Spawning
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                 DelegatedPermissions Contract               │
│      AICHAT_CAPABILITY_MASK        AICHAT_AUTHORITY_CEILING │
│      (Child processes NEVER inherit AICHAT_AUTONOMY)        │
└─────────────────────────────────────────────────────────────┘
```

---

## 2D Posture Matrix

```
                          AUTHORITY CEILING (Gate 2 / Escalation)
                       Safe          Reversible      Disruptive      Destructive
                  ┌──────────────┬──────────────┬──────────────┬──────────────┐
  readonly (Gate1)│   readonly   │      ──      │      ──      │      ──      │
CAPABILITY MASK   │ (Audit/Triage)              │              │              │
                  ├──────────────┼──────────────┼──────────────┼──────────────┤
  mutating        │   consult    │  reversible  │   Standard   │   Standard   │
                  │ (Supervised) │ (Bounded AP) │    Legacy    │   Default    │
                  └──────────────┴──────────────┴──────────────┴──────────────┘
```

| Level | Root Mask (Gate 1) | Root Ceiling (Gate 2) | Option B Auto-Reversibility | Child Subagent Delegation Cap | Non-TTY / Headless Behavior |
|:---|:---:|:---:|:---:|:---:|:---|
| **`readonly`** | `readonly` | `Safe` | Disabled | Forced `readonly`, `safe` | Fails closed on any mutation |
| **`consult`** | `mutating` | `Safe` | Stepped down to $\ge \text{Reversible}$ (requires human sign-off) | `readonly`, `safe` (triagers only) | Fails closed on any mutation |
| **`reversible`** | `mutating` | `Reversible` | Enabled (atomic `.bak` journal backup) | `mutating`, `reversible` | Auto-applies reversible fixes; halts on disruptive |

---

## The Evaluator-First Unified Human Approval Funnel

```
Action proposed by Agent (Depth 0)
   │
   ▼
[Gate 1: Capability Mask] ──(masked && mutating)──► BLOCKED (capability_denied)
   │                                                 (Zero prompt, zero token cost)
   ▼ (unmasked)
[Gate 2: Protected Policy & Ceiling Check]
   │
   ├─ Policy Forbid ──────────────────────────────► BLOCKED (policy_forbidden)
   │
   ├─ Option B Opportunistic Remediation (Reversible mode only)
   │     └─ Takes preflight atomic backup in journal; steps down to Reversible
   │
   ▼
[Gate 3: %assess-risk% Evaluator] (non-Safe tools)
   │  Evaluator inspects concrete arguments, script source, and intent.
   │  Returns structured RiskVerdict (clamped raise-only, no pardons).
   │
   ▼
Does effective risk exceed authority ceiling OR evaluator flagged Low Confidence/Concerns?
   │
   ├─ NO (Within ceiling) ────────────────────────► EXECUTE & JOURNAL
   │
   └─ YES (Consultation required)
         │
         ▼
[Unified Human Approval Prompt]
   Single interactive prompt displaying:
   • Tool Name & Invocation Arguments
   • Effective Risk vs. Authority Ceiling
   • Evaluator Model, Verdict, & Rationale
   • Active Safeguards (e.g. atomic backup file)
   • Action: [c]ontinue | [h]alt | [r]evert | [e]xplain | [g]uide
         │
         ├─ Continue ─────────────────────────────► EXECUTE & JOURNAL
         └─ Halt / Revert ────────────────────────► ABORT / REVERT
```

---

## Data Models & Seam Map

### 1. `AutonomyLevel` (`src/safety.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutonomyLevel {
    ReadOnly,
    Consult,
    Reversible,
}

impl AutonomyLevel {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "readonly" | "read-only" | "observer" | "a0" => Some(AutonomyLevel::ReadOnly),
            "consult" | "ask" | "copilot" | "a1" => Some(AutonomyLevel::Consult),
            "reversible" | "revert" | "autopilot" | "a2" => Some(AutonomyLevel::Reversible),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AutonomyLevel::ReadOnly => "readonly",
            AutonomyLevel::Consult => "consult",
            AutonomyLevel::Reversible => "reversible",
        }
    }

    pub fn capability_mask(&self) -> Option<&'static str> {
        match self {
            AutonomyLevel::ReadOnly => Some("readonly"),
            AutonomyLevel::Consult | AutonomyLevel::Reversible => None,
        }
    }

    pub fn authority_ceiling(&self) -> AuthorityCeiling {
        match self {
            AutonomyLevel::ReadOnly | AutonomyLevel::Consult => AuthorityCeiling::UpTo(BlastRadius::Safe),
            AutonomyLevel::Reversible => AuthorityCeiling::UpTo(BlastRadius::Reversible),
        }
    }

    pub fn permits_autonomous_reversibility(&self) -> bool {
        match self {
            AutonomyLevel::ReadOnly | AutonomyLevel::Consult => false,
            AutonomyLevel::Reversible => true,
        }
    }
}
```

### 2. Seam Map

| Component | File | Changes |
|---|---|---|
| `AutonomyLevel` enum | `src/safety.rs` | Define enum, `from_str_loose`, mapping helpers for mask, ceiling, and reversibility. |
| Configuration | `src/config/mod.rs` | Add `autonomy: Option<AutonomyLevel>` to `SafetyConfig` (defaults to `Some(AutonomyLevel::ReadOnly)`), env var `AICHAT_AUTONOMY` in `load_envs`. Support `none` to opt out. |
| CLI Argument | `src/cli.rs` | Add `--autonomy <LEVEL>` to `Cli` struct [default: readonly]; apply in `config_override`. |
| Posture Expansion | `src/agent_loop.rs` | Expand root posture at start of `run`/`run_agent`. |
| Reversibility Clamp | `src/agent_loop.rs` | In `authority_denied_result`, check `permits_autonomous_reversibility()` before allowing Option B to auto-pass mutations. |
| Evaluator-First Funnel | `src/agent_loop.rs` | Restructure `eval_single_tool` to run Gate 3 before presenting the unified human prompt on Gate 2 over-ceiling events. |
| Observability Banner | `src/agent_loop.rs` | Emit startup posture banner in progress trace: `[safety posture: <level> — ...]`. |
