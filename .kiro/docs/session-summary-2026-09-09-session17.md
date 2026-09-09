# Session Summary 17: Hierarchical Guide Rails, Semantic Role Badging & Turn Delta Observability

**Date:** 2026-09-09  
**Branch:** `feat/tool-safety-permission-boundary`  
**Test Suite Status:** 494 tests passing (486 unit/integration + 5 catalog override + 3 web assets, 0 failed); clippy clean; release binary built.  
**Specification:** `.kiro/specs/tool-safety-modes/requirements.md` (`FR-6d.25`), `tasks.md` (`Task 6d.33`–`Task 6d.36`)

---

## 1. Executive Summary

In Session 17, we address visual clutter, the optical illusion of "repetitive querying / loops", and terminal line wrapping bugs experienced when scrolling through `--dialog --no-truncate` execution traces.

When multi-agent loops run with `--dialog --no-truncate`, several visual factors impede readability:
1. **Cumulative Chat History:** Because LLM endpoints are stateless, Turn $N$ re-submits all prior turns. Skimming across turns makes Turn 3 look 80% identical to Turn 2.
2. **Verbatim System Prompts:** 30–40 lines of static instruction boilerplate are printed in full brightness on every turn.
3. **Monochrome Role Headers:** Plain text headers (`[system]`, `[user]`, `[assistant]`, `tool_result:`) blend into the payload without syntax cues.
4. **Weak Indentation Hierarchy & Broken Line Wrapping:** Terminal emulators hard-wrap long lines exceeding terminal width at column 0. When long prompts, tool results, JSON arguments, or assistant markdown paragraphs wrapped, continuation lines spilled over to column 0, breaking indentation and cutting directly through the vertical guide rails.

Under **`FR-6d.25`**, we introduce:
- **Color-Coded Vertical Guide Rails:** Distinct vertical rails (`│`, `║`, `╏`) matching each agent's color palette anchor every line of output to its parent/child execution frame.
- **Asymmetric Request vs. Response Framing:** Distinct iconography (`📥 PROMPT` vs `📤 RESPONSE`) and directional styling clearly differentiate LLM inputs from generations.
- **Semantic Role Badging & Dimming:** `[user]` (cyan), `[assistant]` (yellow), `tool_calls:` (light blue), and `tool_result:` (magenta) are color-badged. Static instructions are dimmed.
- **Multi-Turn System Prompt Folding:** On `turn > 1`, static instructions are collapsed to `[system: <N> lines instructions unchanged]` to eliminate 70% of screen height clutter.
- **Turn Delta Highlighting:** Previous turns' messages are styled as `[history: ...]` (dimmed), while newly appended turn inputs are highlighted with `⚡ [new: tool_result: ...]`.
- **ANSI-Aware Soft-Wrapping & Guide Rail Continuity:** Long lines are soft-wrapped at word boundaries to fit the inner terminal column width. Every wrapped line preserves the vertical guide rail (`line_prefix`), context-appropriate hanging indents (code/JSON/badges/bullets), and open ANSI escape styles across lines.
- **Responsive Terminal Box Borders:** Header and footer box borders (`┌── ... ───`, `└── ┄┄┄`) dynamically calculate visible width and clamp to terminal width, preventing trailing delimiter overflow.

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
- ANSI escape codes have zero visible width and are never split across wrapped lines.

---

## 3. Implementation Details

1. **`src/agent_loop.rs` (`format_dialog_block`, `emit_dialog_block`):**
   - Renders multi-tier vertical guide rails (`│`, `║`, `╏`) matching `agent_color`.
   - Distinguishes requests (`📥 [pid agent [turn N/M] PROMPT SUBMITTED TO LLM]`) from responses (`📤 [pid agent [turn N/M] RESPONSE FROM LLM]`).
   - Dynamically sizes header borders (`┌── ... ───`) and footer borders (`└── ┄┄┄`) to fit terminal columns without overflow.
   - Wraps content lines with `wrap_ansi_line`, ensuring all continuation lines prepend vertical guide rails.
2. **`src/agent_loop.rs` (`wrap_ansi_line`, `strip_ansi`, `visible_width`, `detect_continuation_indent`):**
   - Implemented zero-copy ANSI tokenizer that differentiates SGR escapes, whitespace, and words.
   - Tracks active ANSI styles across line boundaries, ensuring clean resets and style restoration.
   - Detects leading whitespace, bullets (`* `, `- `), and badges (`⚡`, `[history:`, `tool_result:`) to apply context-appropriate hanging indents.
3. **`src/agent_loop.rs` (`handle_agent_loop_progress`):**
   - Progress trace events (`[pid assess-risk: ...`, `[pid fs_create completed ...]`) maintain ancestor guide rails (`ancestor_rails(depth)`) and soft-wrap long descriptions within terminal width.
4. **`src/agent_loop.rs` (`format_messages_dialog`, `format_messages_dialog_with_turn`):**
   - Contextualizes formatting using loop `turn`.
   - Folds static system prompt boilerplate on `turn > 1` (`[system: <N> lines instructions unchanged]`).
   - Dims static instructions on `turn == 1`.
   - Semantic color badging for roles (`[user]`, `[assistant]`, `tool_calls:`, `tool_result:`).
   - Labels prior turn messages as `[history: ...]` while highlighting new turn deltas with `⚡ [new: tool_result: <tool>] ->`.

---

## 4. Verification & Validation Results

1. **Unit & Integration Test Suite:**
   - Run: `cargo test --all-targets`
   - Result: **494 passed, 0 failed** (486 binary tests + 5 catalog override tests + 3 web asset tests).
   - Validated tests:
     - `test_strip_ansi_and_visible_width`
     - `test_wrap_ansi_line_plain_text`
     - `test_wrap_ansi_line_with_ansi_styling`
     - `test_format_dialog_block_wraps_long_lines_with_guide_rails`
     - `test_format_dialog_block_nested_child_rails_and_box_containment`
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
     - JSON payloads and tool invocation strings soft-wrap with hanging indents inside guide rails.
   - `nu scripts/run-demos.nu --demo 4 --dialog --no-truncate`:
     - Long multi-sentence web search paragraphs and summaries soft-wrap within the rails.
     - Guide rails remain completely unbroken, continuous, and aligned; zero lines wrap to column 0.
   - `nu scripts/run-demos.nu --demo 5 --dialog --no-truncate`:
     - Dual researcher calls and synthesis rendered cleanly across turns without visual clutter or indentation clashing.
