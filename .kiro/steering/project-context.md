# Project Context: Project Perry (Agent P)

This is the enhanced `aichat` fork, codenamed **Perry** (in honor of *Agent P*).

> **Codename: Perry (Agent P)**  
> Deceptively compact, mild-mannered, and provider-agnostic on the outside (runs as a static musl binary on a 64MB bastion host); put on the fedora (`agent: true`) and it becomes an elite, undercover agentic SRE harness. Its core operational mission: disarming catastrophic infrastructure "-Inators" through deterministic actuation governance, process-isolated subagents, mTLS supervisory escalation to Major Monogram (the human-in-the-loop / orchestrator), durable rollback journals, and progressive runbook disclosure without ever granting an LLM "pardoning" power.

> [!IMPORTANT]
> **Canonical Glossary Mandate:** All agents operating in this repository MUST consult and adhere to [`.kiro/docs/glossary.md`](../docs/glossary.md). The terminology used in Perry (such as `ImpactTier` vs `AuthorityCeiling`, `Deterministic Floor`, `Option B Pre-flight Remediation`, and `HumanReserved`) has precise technical semantics distinct from generic LLM/DevOps colloquialisms.


## Build & Test

- Build: `cargo build`
- Release build: `cargo build --release`
- Run tests: `cargo test`
- Run a specific test: `cargo test <test_name>`
- Check without building: `cargo check`
- Lint: `cargo clippy`

## Key Info

- Rust edition: 2021
- Async runtime: Tokio (multi-threaded)
- CLI parsing: clap (derive)
- HTTP client: reqwest
- HTTP server: hyper
- REPL: reedline
- Config format: YAML (serde_yaml)
- Logging: simplelog

## Architecture Reference

See #[[file:.kiro/architecture.md]] for full architecture documentation.

## Local Environment Layout

This system has both a production (installed) and development setup:

### Perry Local Environment
- **Binary:** `~/.local/bin/perry` (symlinked from `~/projects/perry/target/release/perry`)
- **Config Root:** `~/.config/perry/config.yaml` (independent from legacy `~/.config/aichat/`)
- **Functions:** `~/.config/perry/functions -> ~/projects/innators`
- **Default Autonomy Posture:** `--autonomy readonly` (least privilege baseline; opt out with `--autonomy none`)
- **Web Search Model Fallback:** Configurable via `web_search_model:` in `config.yaml` or `WEB_SEARCH_MODEL` / `PERRY_WEB_SEARCH_MODEL` env vars (falls back to `model:` in `config.yaml`)

### Development Workspaces
- **Perry source:** `~/projects/perry` (primary development, tracking `fujibearly/Perry.git:main`)
- **Innators source:** `~/projects/innators` (companion actuation tools, tracking `fujibearly/innators.git:main`)

### Historical / Upstream Backups
- Upstream `/usr/bin/aichat` and `~/clones/llm-functions` remain preserved as legacy references.
- Local backups in `~/projects/aichat` and `~/projects/llm-functions` are read-only.
- The `~/projects/innators` clone is the active actuator layer for Perry.
- Dual subprocess injection ensures tools executing `aichat` inherit `AICHAT_CONFIG_DIR=~/.config/perry`.

## Conventions

- New LLM providers are added via the `register_client!` macro in `src/client/mod.rs`
- Client implementations go in `src/client/<provider>.rs`
- All clients implement the `Client` trait from `src/client/common.rs`
- Config fields use `serde(default)` and are loaded from YAML + environment variables
- Environment variable names follow the pattern `AICHAT_<KEY>` (uppercase, underscored)
- The `GlobalConfig` type (`Arc<RwLock<Config>>`) is passed through most async functions
- Tool/function binaries are external processes invoked via `run_command()`
