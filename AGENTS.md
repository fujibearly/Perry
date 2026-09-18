# Agent Guidelines: Project Perry (Agent P)

## 1. Project Overview
- **Project Name:** Perry (Agent P)
- **Repository:** `https://github.com/fujibearly/Perry.git` (origin)
- **Nature:** High-performance, provider-agnostic Rust LLM CLI and agentic SRE harness (continued from `aichat`).
- **Companion Repo:** Actuator tools and subagent personas live in [`/home/istari/projects/innators`](file:///home/istari/projects/innators) (`https://github.com/fujibearly/innators.git`).
- **Historical Backups:** Legacy `/home/istari/projects/aichat` and `/home/istari/projects/llm-functions` are read-only backups.

---

## 2. Source of Truth Documentation
Before proposing or implementing architectural changes, always consult:
1. [`.kiro/architecture.md`](file:///home/istari/projects/perry/.kiro/architecture.md) — System state machine, supervisory escalation, permission boundaries, and safety taxonomy.
2. [`lesssons-learned.md`](file:///home/istari/projects/perry/lesssons-learned.md) — Critical debugging lessons, UI formatting quirks, and REPL handling.
3. [`.kiro/steering/project-context.md`](file:///home/istari/projects/perry/.kiro/steering/project-context.md) — Local development conventions and environment variables.
4. [`.kiro/docs/glossary.md`](file:///home/istari/projects/perry/.kiro/docs/glossary.md) — Canonical definitions of `ImpactTier`, `AuthorityCeiling`, and `Deterministic Floor`.

---

## 3. Nushell Scripting Rules (for `scripts/`)
When writing or editing Nushell scripts (e.g. `scripts/run-demos.nu`):
- **Avoid Functional Loops in Tight Loops**: Replace nested closures (`.each`, `.filter`) with procedural keywords (`for`, `match`) to avoid VM stack frame allocations.
- **Structural Pattern Matching**: Prefer `match` destructuring over nested optional paths (`?.`) and pipeline defaults (`| default ...`).
- **Single-Pass Record Modification**: Combine sequential `upsert` calls into single `merge` calls.
- **Short-Circuit String Parsing**: Add cheap conditional guards (`str contains`) before executing string splitting (`split row`, `str trim`).

---

## 4. Testing & Verification
- Check compilation: `cargo check`
- Run unit & integration tests: `cargo test`
- Run demo verification harness: `nu scripts/run-demos.nu`
- Companion functions override: `export AICHAT_FUNCTIONS_DIR=/home/istari/projects/innators`
