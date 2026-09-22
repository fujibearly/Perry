# Session 31: Dedicated ~/.config/perry Migration, Default --autonomy readonly (Method 2), Hierarchical WEB_SEARCH_MODEL Resolution & Actuator Decoupling

**Period:** `2026-09-22`  
**Repositories:**  
- **Perry:** `https://github.com/fujibearly/Perry.git` $\rightarrow$ `/home/istari/projects/perry` (Engine)  
- **Innators:** `https://github.com/fujibearly/innators.git` $\rightarrow$ `/home/istari/projects/innators` (Actuators)  
**Handoff Document:** `.kiro/docs/session-summary-2026-09-22-session31.md`  
**Consolidated Index Entry:** `SESSION_SUMMARY.md` (#31)

---

## 1. Executive Summary

Session 31 established operational independence for Project Perry and hardened its security boundaries:
1. **Dedicated Configuration Directory (`~/.config/perry/`):** Fully decoupled Perry from upstream `~/.config/aichat/`. Established independent configuration root at `~/.config/perry/` with active `config.yaml`, roles, sessions, and a symlink (`functions -> ~/projects/innators`). Symlinked `target/release/perry` to `~/.local/bin/perry` for system-wide CLI access.
2. **Default Operational Autonomy Posture (Method 2: `--autonomy readonly`):** Enforced the principle of least privilege at the engine level by setting `SafetyConfig.autonomy = Some(AutonomyLevel::ReadOnly)` by default. Autonomous runs now fail closed against mutating operations unless explicitly relaxed by the operator (`--autonomy none` or `unrestricted`). Fine-grained authority overrides remain strictly isolated.
3. **Hierarchical `WEB_SEARCH_MODEL` Resolution & Actuator Interop:** Eliminated tool aborts caused by missing environment variables. Relaxed `argc` declaration in `web_search_aichat.sh` from mandatory (`!`) to optional. Added `web_search_model: Option<String>` to Perry's `Config` schema, and implemented automatic environment injection in `eval_shell` (`PERRY_CONFIG_DIR`, `AICHAT_CONFIG_DIR`, and `WEB_SEARCH_MODEL`).
4. **Distillation Accounting & Artifact Defanging:** Fixed telemetry distillation line accounting (`lines_in -> lines_out`), corrected live progress truncation, and defanged dumped diagnostic scripts (`.sh` $\rightarrow$ `.txt`, `chmod 600`) to prevent accidental execution.

---

## 2. Core Architectural Decisions

### 2.1 Dedicated Configuration Directory Decoupling
- **Problem:** Perry was relying on fallback detection to `~/.config/aichat/`. Running Perry from release target directories prompted interactively for configuration creation if the binary was not installed in PATH, and actuator child tools calling `aichat` looked for `~/.config/aichat/`.
- **Decision:**
  - Migrated configuration to a clean dedicated directory `~/.config/perry/`.
  - Linked `~/.config/perry/functions -> /home/istari/projects/innators`.
  - In [`src/function.rs`](file:///home/istari/projects/perry/src/function.rs#L878), `eval_shell` injects `PERRY_CONFIG_DIR` and `AICHAT_CONFIG_DIR` pointing to `~/.config/perry` into all tool subprocess environments, ensuring legacy wrappers resolve configuration cleanly.
  - Symlinked `./target/release/perry` to `~/.local/bin/perry`.

### 2.2 Default Operational Autonomy Posture (Method 2)
- **Problem:** Without an explicit `--autonomy` flag, agent loops ran with unconstrained autonomy (`autonomy: None`), leaving Gate 2 ceilings at `Destructive`.
- **Decision:**
  - Set `SafetyConfig::default().autonomy = Some(AutonomyLevel::ReadOnly)`.
  - Supported `--autonomy none` (and aliases `unrestricted`, `off`, `full`) to allow operators to explicitly clear the macro posture.
  - Preserved raw fine-grained authority ceiling isolation in tests by setting `autonomy: None` in testing harnesses.

### 2.3 Hierarchical Fallback Resolution for Actuator Tool Dependencies
- **Problem:** Actuator scripts declaring `# @env WEB_SEARCH_MODEL!` aborted before execution if the operator omitted `export WEB_SEARCH_MODEL=...`, despite valid models being configured in `config.yaml`.
- **Decision:**
  - Relaxed `# @env WEB_SEARCH_MODEL!` to `# @env WEB_SEARCH_MODEL` in `innators/tools/web_search_aichat.sh`.
  - Implemented 4-tier resolution order:
    1. Explicit environment variable (`WEB_SEARCH_MODEL` or `PERRY_WEB_SEARCH_MODEL`)
    2. Config override (`web_search_model:` in `~/.config/perry/config.yaml`)
    3. Primary config model (`model:` in `config.yaml`)
    4. Stable default (`gemini:gemini-2.5-flash`)
  - Perry's `eval_shell` injects resolved `WEB_SEARCH_MODEL` into child tool environments if unset.

---

## 3. Verification & Live Test Results

| Test / Verification | Command | Results | Status |
| :--- | :--- | :--- | :---: |
| **Unit & Integration Suite** | `cargo test -- --test-threads=1` | 559 unit/integration + 5 catalog + 3 web asset security tests | **567 passed; 0 failed** |
| **Web Search Model Resolution** | `cargo test config::tests::test_web_search_model` | Verified 3-tier fallback and YAML deserialization | **Passed** |
| **Autonomy Precedence Tests** | `cargo test test_autonomy_posture_precedence` | Macro vs fine-grained ceiling precedence | **Passed** |
| **Sysinfo Display** | `perry --info` | Verified `web_search_model` and `config_file` point to `~/.config/perry/` | **Passed** |
| **Actuator Tool Standalone** | `env -u WEB_SEARCH_MODEL tools/web_search.sh --links --query "Rust async"` | Discovered search links without required env abort | **Passed** |
| **SRE Agent Dry-Run** | `perry --agent sre --dry-run "Check system health"` | Verified prompt composition, skills, and tools | **Passed** |

---

## 4. Repository Status & Commits

- **Perry Engine (`/home/istari/projects/perry`):**
  - Branch: `main` (tracked to `https://github.com/fujibearly/Perry.git`)
  - Commit `b7654da`: Engine-level default `--autonomy readonly` (Method 2)
  - Commit `d5ca6ec`: `web_search_model` configuration and automatic tool env fallback
  - Commit `9bdad8d`: Lessons learned, project context, and autonomy ladder specifications
  - Status: Clean, pushed to `origin/main`.
- **Innators Actuators (`/home/istari/projects/innators`):**
  - Branch: `main` (tracked to `https://github.com/fujibearly/innators.git`)
  - Commit `dff7507`: Make `WEB_SEARCH_MODEL` optional and fall back to `config.yaml`
  - Commit `9916f3b`: Document environment variables and synchronize `sre` schema
  - Status: Clean, pushed to `origin/main`.
