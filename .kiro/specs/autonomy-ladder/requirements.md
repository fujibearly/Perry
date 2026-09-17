# Autonomy Ladder (`--autonomy`) — Requirements

## Context & Problem Statement
Project Perry's tool safety architecture (Backlog #6a–#6d) provides a rigorous, multi-layered defense-in-depth framework:
1. **Capability Mask (#6a):** Binary process sandbox boundary (`AICHAT_CAPABILITY_MASK=readonly` vs. unmasked).
2. **Authority Ceiling (#6b):** Ordered 5-tier blast-radius boundary (`Safe < Reversible < Disruptive < Destructive < Catastrophic`).
3. **Proven Reversibility (#6b/#6d):** Reversibility credit via declared properties or Option B atomic journal backups.
4. **Risk Evaluator (#6c):** Monotonic, stricter-only `%assess-risk%` LLM overlay.
5. **Process Sandboxing & Control (#6d):** Hard child sandbox boundaries, zero downward permit propagation, and root-only human escalation.

While mechanically complete, operating this system requires configuring multiple fine-grained knobs (`AICHAT_CAPABILITY_MASK`, `AICHAT_AUTHORITY_CEILING`, `safety.default_ceiling`, `safety.policy_file`). Operators across coding, systems, and research need clean, domain-agnostic operational postures that reflect the exact technical boundaries:
- **`readonly`:** Strict read-only observation and audit; zero mutation permitted.
- **`consult`:** Supervised pair workflow; read operations auto-execute; all mutations consult the human operator.
- **`reversible`:** Bounded execution; low-impact, reversible actions auto-execute with rollback artifacts; irreversible or dangerous actions consult the human operator.

---

## Functional Requirements

### FR-19.1 — AutonomyLevel Nomenclature & Parsing
- The engine MUST define:
  ```rust
  pub enum AutonomyLevel {
      ReadOnly,
      Consult,
      Reversible,
  }
  ```
- The parser MUST accept human-readable names (`readonly`, `consult`, `reversible`, case-insensitive).
- The parser MUST accept shorthand aliases:
  - `a0`, `observer` $\to$ `ReadOnly`
  - `a1`, `copilot` $\to$ `Consult`
  - `a2`, `autopilot` $\to$ `Reversible`
- **Default:** If `--autonomy` is omitted, the engine remains backwards compatible, falling back to `safety.default_ceiling` (defaulting to `Destructive`) and leaving the root process unmasked.

### FR-19.2 — 2D Posture Decomposition (Root Macro Expansion)
`AutonomyLevel` MUST expand at root process startup into the two canonical safety axes:
1. **`readonly`:**
   - Capability Mask: `readonly` (Gate 1 active).
   - Authority Ceiling: `AuthorityCeiling::UpTo(BlastRadius::Safe)` (Gate 2).
   - Reversibility Credit: Disabled.
   - Guarantee: All mutating tools fail closed at Gate 1 (`capability_denied`) without human prompt or LLM evaluator call. Read-only tools execute autonomously iff impact is `Safe`.
2. **`consult`:**
   - Capability Mask: unmasked (`mutating` allowed in Gate 1).
   - Authority Ceiling: `AuthorityCeiling::UpTo(BlastRadius::Safe)` (Gate 2).
   - Reversibility Credit: Stepped down to $\ge \text{Reversible}$ only (mutations MUST NOT auto-execute; they must trip the ceiling).
   - Guarantee: Read-only tools auto-execute; all mutating tools trip the ceiling and consult the human operator for approval.
3. **`reversible`:**
   - Capability Mask: unmasked (`mutating` allowed in Gate 1).
   - Authority Ceiling: `AuthorityCeiling::UpTo(BlastRadius::Reversible)` (Gate 2).
   - Reversibility Credit: Enabled (Option B preflight atomic backup in durable rollback journal).
   - Guarantee: Reversible mutations auto-execute with verified journal backups; Disruptive, Destructive, Catastrophic, or Unclassified actions halt and consult the human operator for approval.

### FR-19.3 — Configuration & Precedence Hierarchy (Zero Env Var Soup)
- Configuration entry: `safety.autonomy: <readonly|consult|reversible>`.
- Environment variable: `AICHAT_AUTONOMY=<readonly|consult|reversible>`.
- CLI flag: `--autonomy <LEVEL>`.
- **Precedence:** CLI `--autonomy` > Env `AICHAT_AUTONOMY` > Config `safety.autonomy`.
- **Fine-Grained Overrides:** Explicit fine-grained flags (e.g. `--ceiling <tier>` or `AICHAT_SAFETY_DEFAULT_CEILING`) override the autonomy baseline ceiling if explicitly provided.
- **Child Subagent Sandboxing:** Child subagent processes MUST NEVER inherit `AICHAT_AUTONOMY`. Child processes receive only their explicit, per-process sandbox channels (`AICHAT_CAPABILITY_MASK` and `AICHAT_AUTHORITY_CEILING`) provisioned via `DelegatedPermissions`.

### FR-19.4 — Subagent Delegation Postures (FR-6d.18 / FR-6d.24 Compliance)
- Subagents CANNOT prompt the human on `/dev/tty` and CANNOT elevate permissions in-flight.
- **In `readonly`:** Parent is masked `readonly` and capped at `Safe`. In accordance with `validate_against_parent`, subagents are strictly clamped to `readonly` and `Safe`.
- **In `consult`:** Subagents default strictly to `mask: readonly, ceiling: safe` (triagers/sensors only). Any mutation proposed by a subagent fails closed with `permission_blocked`. The root orchestrator receives this finding and actuates the mutation directly while consulting the human operator.
- **In `reversible`:** Subagents may be provisioned upfront with `mask: mutating, ceiling: reversible`. Subagents auto-execute reversible operations. Actions exceeding `Reversible` fail closed, unwind, and report `permission_blocked` to the root orchestrator.

### FR-19.5 — Evaluator-First Unified Human Consultation Funnel
- In `eval_single_tool`, if an action exceeds the authority ceiling (Gate 2), the engine MUST NOT prompt the human immediately with incomplete information.
- The engine MUST execute the `%assess-risk%` evaluator (Gate 3) first (if configured and non-Safe) to extract semantic risk findings, concrete argument analysis, and rationale.
- If human authorization is required (due to Gate 2 over-ceiling OR Gate 3 risk raise):
  - The engine MUST present **one unified, fully-informed prompt** to the operator containing both the authority comparison and the evaluator's risk rationale.
  - If the operator approves (`Continue`), the action proceeds directly to journal recording and actuation without duplicate prompts.
  - If the operator rejects (`Halt` or `Revert`), actuation is aborted immediately.

### FR-19.6 — Observability & Trace Formatting
- At session startup, if an autonomy posture is active, the engine MUST emit an informational trace banner:
  `[safety posture: <level> — capability_mask=<mask_val>, authority_ceiling=<ceiling_val>]`.
- Pass/Block grammar across traces MUST continue to strictly satisfy FR-6d.27 (`ALLOW <tool>: risk <tier> <= ceiling <tier>` and `BLOCK <tool>: risk <tier> > ceiling <tier>`).

---

## Non-Functional Requirements

- **NFR-1 — Zero Regressions:** All existing 529 tests MUST pass.
- **NFR-2 — Bastion Portability:** Zero new crate dependencies. Musl static binary footprint impact < 15KB.
- **NFR-3 — Deterministic Fail-Closed:** In non-interactive or headless execution (piped input, CI/CD, cron), any action requiring human consultation MUST fail closed immediately with a structured refusal payload.
