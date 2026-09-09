# Session Summary 17: Hierarchical Guide Rails, Semantic Role Badging & Turn Delta Observability

**Date:** 2026-09-09  
**Branch:** `feat/tool-safety-permission-boundary`  
**Test Suite Status:** 486 tests passing baseline; refactoring in progress for FR-6d.25.  
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

## 3. Implementation Tasks Planned

1. **`src/agent_loop.rs` (`format_dialog_block`, `emit_dialog_block`):**
   - Draw colored vertical guide rails based on recursion depth and `agent_color`.
   - Implement asymmetric framing for `DialogDirection::Request` (`📥 PROMPT`) vs `DialogDirection::Response` (`📤 RESPONSE`).
2. **`src/agent_loop.rs` (`format_messages_dialog`):**
   - Accept `turn: usize` context.
   - Fold static system prompt on `turn > 1` (`[system: <N> lines instructions unchanged]`).
   - Dim static instructions on `turn == 1`.
   - Apply semantic color badging for roles (`[user]`, `[assistant]`, `tool_calls:`, `tool_result:`).
   - Differentiate `[history]` from `⚡ [new]` messages.
3. **Testing & Verification:**
   - Update unit tests in `src/agent_loop.rs`.
   - Verify `cargo test` (486 tests) and `cargo clippy`.
   - Run live demos (`Demo 3`, `Demo 4`, `Demo 5`) with `--dialog --no-truncate` to visually confirm clean, scannable traces without repetition.
