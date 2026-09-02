# Code Coverage Evaluation Report

**Timestamp:** 2026-08-31T11:36:50-04:00  
**Target Binary:** `aichat v0.31.0-fork.9` (`target/release/aichat`)  
**Evaluation Harness:** `target/release/run-demos.nu` (11 E2E multi-agent demo scenarios)  
**Profiling Engine:** LLVM Source-Based Code Coverage (`RUSTFLAGS="-C instrument-coverage"`, `llvm-profdata`, `llvm-cov`)  
**Total Process Traces Merged:** 17 parent and child agent processes

---

## 1. Executive Summary

A full end-to-end dynamic code coverage assessment was performed across the fork's new capabilities by compiling the release binary with LLVM coverage instrumentation, running the complete Nushell demo suite, and merging execution profiles across all spawned sub-agents and subprocesses.

### Core Agent Loop (`src/agent_loop.rs`)
* **Function Coverage:** **79.41%** (54 / 68 functions)
* **Line Coverage:** **72.90%** (573 / 786 lines)
* **Region Coverage:** **71.87%** (856 / 1,191 regions)

---

## 2. Subsystem Coverage Breakdown

| Component | Source File | Lines | Line Coverage | Function Coverage | Covered Code Paths |
| :--- | :--- | :---: | :---: | :---: | :--- |
| **Agent Loop** | `src/agent_loop.rs` | 786 | **72.90%** | **79.41%** | Iterative turns, semaphore parallel tools, `_plan` scratchpad, subprocess sub-agents, auto-capping, pipe routing, file routing, cost accumulation. |
| **CLI Parameter Ingestion** | `src/cli.rs` | 40 | **67.50%** | **80.00%** | Multi-agent mode flags, trace toggling, cost budgeting, agent selection. |
| **Role & Persona Resolution** | `src/config/role.rs` | 264 | **56.44%** | **53.49%** | `%functions%` virtual role injection, prompt structure parsing. |
| **Agent Configurations** | `src/config/agent.rs` | 431 | **38.75%** | **45.76%** | Cognitive agent resolution (`orchestrator`, `researcher`, `coder`), private tool bindings. |
| **Function Dispatch** | `src/function.rs` | 329 | **37.39%** | **40.48%** | Declarative schema validation, tool registration, output routing parsing. |
| **Token & Cost Accounting** | `src/client/stream.rs` | 514 | **31.91%** | **37.50%** | Multi-round token usage accumulation, USD pricing tracking. |
| **Observability & Spinnners** | `src/utils/spinner.rs` | 209 | **33.49%** | **54.55%** | Progress heartbeat, live `/dev/tty` formatting, OSC title updates. |

---

## 3. Verified Execution Paths

1. **Parallel Concurrency:** Verified concurrent execution of `slow_task` via `tokio::join_all` and semaphore controls.
2. **Turn Budgets:** Verified bounded loop halting and warning emission when `max_turns` is reached.
3. **Planning Tool (`_plan`):** Verified in-process scratchpad capture, live `/dev/tty` trace emissions, and output redaction.
4. **Sub-Agent Subprocess Spawning:** Verified child process isolation, PID tracking, and recursive depth limiting.
5. **Auto-Capping:** Verified offloading $>16\text{ KB}$ outputs (`/usr/share/dict/cracklib-small`) to disk with preview handles.
6. **Pipe Routing:** Verified chained execution (`fetch_url_via_curl` $\rightarrow$ `summarize_text`) without intermediary LLM round-trips.
7. **File Destination:** Verified direct-to-disk artifact creation (`/tmp/generate_data-*.csv`) with confirmation receipts.
8. **Structured Markdown PDF Ingestion:** Verified `pdf2md` page extraction and Markdown conversion (`manual.pdf`).

---

## 4. Coverage Expansion Targets

Identified areas for unit test expansion:
* Edge cases in output routing cycle detection (recursive pipe aborts).
* Error paths in sub-agent crash isolation and non-zero exit codes.
* Strict cost budget (`max_cost`) mid-turn cancellation.
* Deeply nested sub-agent depth boundary enforcement (`max_agent_depth` edge conditions).
