# Session Summary 17: Hierarchical Guide Rails, Semantic Role Badging & Turn Delta Observability

**Date:** 2026-09-09  
**Branch:** `feat/tool-safety-permission-boundary`  
**Test Suite Status:** 489 tests passing (481 unit/integration + 5 catalog override + 3 web assets, 0 failed); clippy clean; release binary built.  
**Specification:** `.kiro/specs/tool-safety-modes/requirements.md` (`FR-6d.25`), `tasks.md` (`Task 6d.33`, `Task 6d.34`)

---

## 1. Executive Summary

In Session 17, we address visual clutter and the optical illusion of "repetitive querying / loops" experienced when scrolling through `--dialog --no-truncate` execution traces.

When multi-agent loops run with `--dialog --no-truncate`, four visual factors impede readability:
1. **Cumulative Chat History:** Because LLM endpoints are stateless, Turn $N$ re-submits all prior turns. Skimming across turns makes Turn 3 look 80% identical to Turn 2.
2. **Verbatim System Prompts:** 30–40 lines of static instruction boilerplate are printed in full brightness on every turn.
3. **Monochrome Role Headers:** Plain text headers (`[system]`, `[user]`, `[assistant]`, `tool_result:`) blend into the payload without syntax cues.
4. **Weak Indentation Hierarchy:** A flat 4-space indent (`"    ".repeat(depth)`) without visual guide rails easily gets lost in terminal text wrapping.

Under **`FR-6d.25`**, we introduce:
- **Color-Coded Vertical Guide Rails:** Distinct vertical rails (`│`, `║`, `╏`) matching each agent's color palette anchor every line of output to its parent/child execution frame.
- **Asymmetric Request vs. Response Framing:** Distinct iconography (`📥 PROMPT` vs `📤 RESPONSE`) and directional styling clearly differentiate LLM inputs from generations.
- **Semantic Role Badging & Dimming:** `[user]` (cyan), `[assistant]` (yellow), `tool_calls:` (light blue), and `tool_result:` (magenta) are color-badged. Static instructions are dimmed.
- **Multi-Turn System Prompt Folding:** On `turn > 1`, static instructions are collapsed to `[system: <N> lines instructions unchanged]` to eliminate 70% of screen height clutter.
- **Turn Delta Highlighting:** Previous turns' messages are styled as `[history: ...]` (dimmed), while newly appended turn inputs are highlighted with `⚡ [new: tool_result: ...]`.

---

## 2. Key Architectural Invariants

### 2.1 No Truncation of Execution Facts
Visual improvements and folding must **never** discard or truncate execution facts when `--no-truncate` / `dialog_no_truncate` is active:
- Tool call names and full arguments are preserved.
- Tool execution results (e.g. `web_search` output) are preserved in full.
- Only invariant, static system prompt boilerplate is folded on subsequent turns.

### 2.2 Strict Terminal Styling Compatibility
All styling uses `nu_ansi_term` and respects terminal detection:
- In non-terminal/piped contexts without tty, ANSI escapes degrade gracefully.
- Guide rails maintain structural column alignment across all agent depths.

---

## 3. Implementation Details

1. **`src/agent_loop.rs` (`format_dialog_block`, `emit_dialog_block`):**
   - Renders multi-tier vertical guide rails (`│`, `║`, `╏`) matching `agent_color`.
   - Distinguishes requests (`📥 [pid agent [turn N/M] PROMPT SUBMITTED TO LLM]`) from responses (`📤 [pid agent [turn N/M] RESPONSE FROM LLM]`).
   - Frames dialog with crisp top/bottom box delimiters (`┌── ... ───` and `└── ┄┄┄`).
2. **`src/agent_loop.rs` (`format_messages_dialog`, `format_messages_dialog_with_turn`):**
   - Contextualizes formatting using loop `turn`.
   - Folds static system prompt boilerplate on `turn > 1` (`[system: <N> lines instructions unchanged]`).
   - Dims static instructions on `turn == 1`.
   - Semantic color badging for roles (`[user]`, `[assistant]`, `tool_calls:`, `tool_result:`).
   - Labels prior turn messages as `[history: ...]` while highlighting new turn deltas with `⚡ [new: tool_result: <tool>] ->`.
3. **`src/agent_loop.rs` (`format_llm_response`):**
   - Highlights tool call dispatches in light blue (`tool_calls: [name(...)]`).
   - Dims empty generation notices.

---

## 4. Verification & Validation Results

1. **Unit & Integration Test Suite:**
   - Run: `cargo test --all-targets`
   - Result: **489 passed, 0 failed** (481 binary tests + 5 catalog override tests + 3 web asset tests).
   - Validated tests:
     - `test_format_messages_dialog_preserves_system_instructions`
     - `test_format_messages_dialog_no_truncate`
     - `test_format_messages_dialog_system_folding_on_turn_2`
     - `test_format_messages_dialog_turn_delta_highlighting`
     - `test_format_dialog_block_rails_and_asymmetric_framing`
2. **Clippy Quality Check:**
   - Run: `cargo clippy --all-targets -- -D warnings`
   - Result: Clean exit (0 warnings, 0 errors).
3. **Optimized Release Build:**
   - Run: `cargo build --release`
   - Result: Successful compilation of release artifact `target/release/aichat`.
4. **Live Terminal Demos:**
   - `nu scripts/run-demos.nu --demo 3 --dialog --no-truncate`:
     - System prompt folded to `[system: 37 lines instructions unchanged]` on turn 2.
     - New tool results highlighted (`⚡ [new: tool_result: fs_create]`).
     - Distinct guide rails clearly delineated nested sub-agent `coder` execution under `orchestrator`.
   - `nu scripts/run-demos.nu --demo 4 --dialog --no-truncate`:
     - Researcher agent web search results rendered with clear distinction between past turns and new tool results.
     - Repetition illusion completely resolved.
   - `nu scripts/run-demos.nu --demo 5 --dialog --no-truncate`:
     - Dual researcher calls and synthesis rendered cleanly across turns without visual clutter.
