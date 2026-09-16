# Project Context: Project Perry (Agent P)

This is the enhanced `aichat` fork, codenamed **Perry** (in honor of *Agent P*).

> **Codename: Perry (Agent P)**  
> Deceptively compact, mild-mannered, and provider-agnostic on the outside (runs as a static musl binary on a 64MB bastion host); put on the fedora (`agent: true`) and it becomes an elite, undercover agentic SRE harness. Its core operational mission: disarming catastrophic infrastructure "-Inators" through deterministic actuation governance, process-isolated subagents, mTLS supervisory escalation to Major Monogram (the human-in-the-loop / orchestrator), durable rollback journals, and progressive runbook disclosure without ever granting an LLM "pardoning" power.

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

### Production (live system)
- **Binary:** `/usr/bin/aichat` (v0.30.0)
- **Config:** `~/.config/aichat/config.yaml`
- **Functions (live):** `~/clones/llm-functions` — already built (`functions.json`, `bin/`, `tools.txt`, `agents.txt` present). Symlinked from `~/.config/aichat/functions`.

### Development (this workspace)
- **aichat source:** `~/projects/aichat` (fork, rc-branch = v0.31.0-fork.9)
- **llm-functions source:** `~/projects/llm-functions` — clean clone for study/development, NOT linked to the live system.

### Important notes
- Do NOT modify `~/clones/llm-functions` without explicit permission — it's the live functions directory used by the installed aichat.
- The `~/projects/llm-functions` clone is safe to experiment with.
- To test dev-built aichat with dev llm-functions: `export AICHAT_FUNCTIONS_DIR=~/projects/llm-functions`
- The installed aichat at `/usr/bin/aichat` may hang on `--info` if run non-interactively (it prompts for config creation).

## Conventions

- New LLM providers are added via the `register_client!` macro in `src/client/mod.rs`
- Client implementations go in `src/client/<provider>.rs`
- All clients implement the `Client` trait from `src/client/common.rs`
- Config fields use `serde(default)` and are loaded from YAML + environment variables
- Environment variable names follow the pattern `AICHAT_<KEY>` (uppercase, underscored)
- The `GlobalConfig` type (`Arc<RwLock<Config>>`) is passed through most async functions
- Tool/function binaries are external processes invoked via `run_command()`
