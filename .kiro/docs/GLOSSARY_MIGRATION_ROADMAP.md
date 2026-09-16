# Glossary & Semantic Types Migration Roadmap
*(Project Perry / Hardened `aichat` Fork)*

> **Canonical Reference:** [`.kiro/docs/glossary.md`](glossary.md)  
> **Status:** Active Tracking Document  
> **Scope:** Safe, incremental migration schedule for semantic type alignment across Project Perry.

---

## 1. Overview & Strategy

To avoid destabilizing hot safety paths and multi-agent coordination during high-velocity development, semantic type improvements in Project Perry follow a 3-tier evolutionary strategy:

1. **Tier 1 (Immediate / Grounded):** Documentation & steering alignment via canonical [`.kiro/docs/glossary.md`](glossary.md), disambiguating 7 domains and 8 historical ambiguities. *(Completed in Commit 1)*
2. **Tier 2 (Surgical / Non-breaking):** Introduction of the transparent [`ImpactTier`](file:///home/istari/projects/aichat/src/function.rs) newtype, [`permits_impact`](file:///home/istari/projects/aichat/src/safety.rs) domain helper, and hot-path gating migration without breaking wire formats or existing APIs. *(Completed in Commits 2–4)*
3. **Tier 3 (Deferred / Opportunistic):** Planned refactorings of legacy enum identifiers and broad call sites scheduled alongside relevant backlog feature milestones. *(Tracked herein)*

---

## 2. Planned Deferred Refactorings

| Legacy Identifier | Target Canonical Identifier | Target Milestone / Scope | Rationale & Migration Strategy |
| :--- | :--- | :--- | :--- |
| `SafetyClass` | `CapabilityMask` | **Backlog #6a / Multi-Agent Polish** | `SafetyClass` conflates safety classification with process-level tool capability filtering (`Readonly` vs `Mutating`). When capability provisioning is next refactored, rename `SafetyClass` $\to$ `CapabilityMask` while preserving serde aliases for backward compatibility. |
| `BlastRadius` (underlying enum) | `IntrinsicImpact` or remain as core scale | **Backlog #12 / Hardening Pass** | `BlastRadius` describes the 5-point ordinal scale (`Safe` $\to$ `Catastrophic`). While `ImpactTier` wraps it to emphasize the impact axis vs authority axis, full underlying enum renaming will occur when AST serialization formats undergo their next major revision. |
| Broad call-site migration of `BlastRadius` $\to$ `ImpactTier` | System-wide usage of `ImpactTier` | **Incremental across Backlogs #8, #9, #10** | Any new modules or functions touching action blast radii should accept or return `ImpactTier`. Existing call sites in telemetry and UI will migrate incrementally as those subsystems are touched. |
| `AuthorityCeiling::UpTo(BlastRadius)` | `AuthorityCeiling::UpTo(ImpactTier)` | **Backlog #6d Phase 2** | Once all tools and evaluator outputs standardize on `ImpactTier`, `AuthorityCeiling`'s internal storage can transition to `ImpactTier` natively. |

---

## 3. Deprecation Schedule & Invariants

1. **Wire-Format Transparency:**
   - Any newtype or renamed type representing tool risk must maintain `#[serde(transparent)]` or serde alias compatibility (`"safe"`, `"reversible"`, `"disruptive"`, `"destructive"`, `"catastrophic"`).
2. **Deterministic Ceiling Lowering:**
   - In accordance with the Canonical Glossary, child processes may only inherit or step down authority ceilings. No semantic type migration may loosen this monotonicity invariant.
3. **Separation of Concerns:**
   - Code reviews must enforce the separation between:
     - **Capability Mask:** What tools a process is permitted to invoke (`ToolMode` / `CapabilityMask`).
     - **Authority Ceiling:** The maximum blast radius an agent is allowed to actuate autonomously (`AuthorityCeiling`).
     - **Impact Tier:** The intrinsic destructive potential of an action (`ImpactTier`).
