# Autonomy Ladder (`--autonomy`) — Tasks

Branch: `feat/autonomy-ladder` (off `main`)

---

## Phase 1 — Data Models & Parsing (`src/safety.rs`)

- [x] **19.1** Define `enum AutonomyLevel { ReadOnly, Consult, Reversible }` in `src/safety.rs` with `Serialize`, `Deserialize`, and `#[serde(rename_all = "lowercase")]`.
- [x] **19.2** Implement `from_str_loose(s: &str) -> Option<AutonomyLevel>` supporting `readonly`, `consult`, `reversible`, as well as aliases `a0`, `a1`, `a2`, `observer`, `copilot`, `autopilot`. Implement `as_str(&self) -> &'static str`.
- [x] **19.3** Implement mapping helpers on `AutonomyLevel`:
  - `capability_mask(&self) -> Option<&'static str>`
  - `authority_ceiling(&self) -> AuthorityCeiling`
  - `permits_autonomous_reversibility(&self) -> bool`
- [x] **19.4** Unit tests in `src/safety.rs`:
  - Round-trip parsing for all names and shorthand aliases.
  - Assert correct mask, ceiling, and reversibility mappings for each level.

---

## Phase 2 — Configuration & CLI Integration (`src/config/mod.rs`, `src/cli.rs`)

- [x] **19.5** Add `pub autonomy: Option<AutonomyLevel>` to `SafetyConfig` in `src/config/mod.rs`.
- [x] **19.6** Map environment variable `AICHAT_AUTONOMY` in `config::load_envs`.
- [x] **19.7** Add `--autonomy <LEVEL>` to `Cli` struct in `src/cli.rs` with custom parsing; wire into `Cli::config_override` so CLI flags override config and env vars.
- [x] **19.8** Unit tests in `src/config/mod.rs` and `src/cli.rs`:
  - Config loading from YAML (`safety.autonomy: consult`).
  - Env var override (`AICHAT_AUTONOMY=reversible`).
  - CLI flag override takes top precedence.
  - Explicit `--ceiling` or `AICHAT_SAFETY_DEFAULT_CEILING` overrides the autonomy baseline ceiling.

---

## Phase 3 — Root Macro Expansion & Reversibility Gating (`src/agent_loop.rs`)

- [x] **19.9** In `src/agent_loop.rs` (`run` / `run_agent`), expand root posture:
  - If `ReadOnly`: activate `AICHAT_CAPABILITY_MASK=readonly`.
  - Override root authority ceiling to `level.authority_ceiling()` unless explicit ceiling was supplied.
  - Emit startup trace banner: `[safety posture: <level> — capability_mask=..., authority_ceiling=...]`.
- [x] **19.10** In `authority_denied_result`, gate Option B preflight auto-reversibility on `level.permits_autonomous_reversibility()`:
  - In `consult`, preflight step-down cannot discount below `Reversible` into autonomous execution.
  - In `reversible`, Option B preflight backup functions normally.

---

## Phase 4 — Evaluator-First Unified Human Consultation Funnel (`src/agent_loop.rs`)

- [x] **19.11** Refactor `eval_single_tool` in `src/agent_loop.rs`:
  - Gate 1 (Mask) continues to execute first; masked mutating tools immediately return `capability_denied` with no prompt and no evaluator call.
  - If unmasked and Gate 2 trips `authority_exceeded`, do NOT prompt immediately.
  - Execute `%assess-risk%` evaluator (Gate 3) first to gather semantic risk analysis, argument inspection, and rationale.
  - Present a single, unified `prompt_human_verdict` combining the authority delta and the evaluator's risk rationale.
  - A `Continue` verdict executes directly; `Halt`/`Revert` aborts cleanly.

---

## Phase 5 — Verification, Tests & Demos

- [x] **19.12** Unit & integration tests in `src/agent_loop.rs`:
  - Test `readonly` mode: mutating tool blocked at Gate 1 with `capability_denied`, zero human prompt, zero evaluator token cost.
  - Test `consult` mode: mutating tool trips ceiling, consults evaluator first, presents unified prompt.
  - Test `reversible` mode: reversible tool with Option B backup executes autonomously; destructive tool trips ceiling and prompts human.
  - Test subagent isolation: subagents spawned in `readonly` and `consult` are restricted to `readonly` / `safe`.
- [x] **19.13** Add Demo 24 to `scripts/run-demos.nu` exercising the three postures live end-to-end.
- [x] **19.14** Run full test suite (`cargo test`, 558 passed) and clippy (`cargo clippy -- -D warnings`).
