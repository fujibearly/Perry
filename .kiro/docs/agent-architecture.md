# Agent Architecture: Tools, Deterministic SH Agents, and Cognitive Sub-Agents

This document details the architectural design, dispatch mechanics, and composition patterns of tools and agents within `aichat` and `llm-functions`.

---

## 1. Architectural Taxonomy

```mermaid
graph TD
    subgraph Route 3: Deterministic Tool / SH Agent [Fast & Pure Bash Execution]
        T_Call[LLM Tool Call: fs_cat / add_todo] --> T_Bin[bin/fs_cat or bin/todo]
        T_Bin --> T_Script[tools/fs_cat.sh or agents/todo/tools.sh]
        T_Script --> T_Ret[Executes locally in ~0.05s and returns output]
    end

    subgraph Route 2: Cognitive Sub-Agent [Autonomous Multi-Turn Subprocess]
        A_Call[LLM Tool Call: researcher] --> A_Fork[Spawns aichat --agent researcher]
        A_Fork --> A_Proc[Dedicated Child Process: Unique PID]
        A_Proc --> A_Loop[Multi-turn loop with private tools & constraints]
        A_Loop --> A_Ret[Returns synthesized report to caller]
    end
```

| Type | Examples | Process Model | Execution | Role in Ecosystem |
| :--- | :--- | :--- | :--- | :--- |
| **Atomic Tool** | `fs_cat`, `web_search`, `read_pdf` | Same process / subshell | **Single invocation (0 turns):** Runs script in milliseconds. | Passive actuator / capability. |
| **Deterministic SH Agent** | `todo`, `sql`, `coder` | Executed via bash shim | **Single invocation (0 turns):** Dispatches subcommands (`todo add_todo`). | Multi-tool CLI bundle. |
| **Cognitive Sub-Agent** | `orchestrator`, `researcher` | Dedicated child process | **Multi-turn loop (1–20 turns):** Autonomous LLM reasoning loop with unique PID. | Active reasoner / specialist. |

---

## 2. Dispatch Mechanics in the Agent Loop

When an LLM emits a tool call, `aichat` evaluates the call through a 3-route dispatcher in `src/agent_loop.rs`:

```rust
// Dispatch Logic in src/agent_loop.rs
if call.name == "_plan" {
    // Route 1: In-process Cognitive Scratchpad
    // Emits trace event, does not execute shell command, returns "acknowledged".
} else if func.is_agent && crate::config::list_agents().contains(&call.name) {
    // Route 2: Cognitive Sub-Agent Delegation
    // Spawns child process: `aichat --agent <name> "<prompt>"` with unique PID.
} else {
    // Route 3: Deterministic Shell Execution
    // Resolves binary via PATH (`llm-functions/bin/<name>`) and runs locally.
}
```

### Why the `list_agents()` check is essential
Upstream `aichat` tags individual tool subcommands (like `add_todo`) with `"agent": true` in `functions.json` to route them via the `bin/todo` binary wrapper. By verifying `list_agents().contains(&call.name)`, `aichat` differentiates between:
1. **Subcommand dispatch** (running `bin/todo add_todo` via Route 3).
2. **Sub-agent delegation** (spawning an autonomous `aichat --agent researcher` child process via Route 2).

---

## 3. The Role of `_plan` (In-Process Pseudo-Tool)

`_plan` is a built-in cognitive scratchpad automatically injected into the agent loop:
* **Function Signature:** `_plan(thought: string)`
* **Execution:** Intercepted in-process by the Rust runtime. No shell processes are spawned.
* **Observability:** Emits an `AgentLoopEvent::PlanReceived` event that prints live to the terminal trace (`/dev/tty`) and updates tmux window titles.
* **Context Retention:** Kept in intermediate turn history for subsequent steps, but completely omitted from final user-facing text responses.

---

## 4. Configuration & Source of Truth in `llm-functions`

The system maintains a pure, simple `argc` Bash architecture:

1. **For Tools (`tools/*.sh`):**
   * Write bash script with `# @describe` and `# @option` argc comments.
   * Add to `tools.txt`.
   * `argc build` compiles them to `functions.json` and creates `bin/<tool>` symlinks.
2. **For Deterministic SH Agents (`agents/<name>/tools.sh`):**
   * Define subcommands in `agents/<name>/tools.sh`.
   * `argc build` compiles subcommands to `agents/<name>/functions.json` and creates `bin/<agent>`.
3. **For Cognitive Sub-Agents (`agents/<name>/`):**
   * Persona and constraints defined in `agents/<name>/index.yaml`.
   * Allowed tools listed in `agents/<name>/tools.txt`.
   * `argc build` dynamically compiles `agents/<name>/functions.json`.

---

## 5. Full Agent Operation Loop & Output Routing Pipeline

Below is the complete execution loop spanning turn budgets, observability hooks, parallel tool dispatch, declarative output routing, and cognitive sub-agent subprocess delegation.

*(The standalone diagram definition is also maintained at [agent-loop-operation.mmd](file:///home/istari/projects/perry/.kiro/docs/agent-loop-operation.mmd))*

```mermaid
flowchart TD
    Start(["User Request / Input"]) --> InitLoop["Initialize Agent Loop\n• Budget: max_turns (default 20)\n• Cost Cap: max_cost\n• Depth: max_agent_depth (default 3)"]

    subgraph ITER_LOOP ["Iterative Agent Execution Loop"]
        InitLoop --> UpdateObs["Update Observability Snapshot\n• OSC Terminal Title (/dev/tty)\n• Status File ($XDG_RUNTIME_DIR/aichat-PID.json)\n• Heartbeat Timer (2s)"]
        UpdateObs --> CheckBudget{"Turn / Cost Budget OK?"}

        CheckBudget -- "Exceeded" --> BudgetWarning["Emit Budget Warning / Halt Loop"]
        BudgetWarning --> FinalReturn

        CheckBudget -- "Within Budget" --> LLMCall["Call LLM with Messages + Tool Declarations"]
        LLMCall --> TrackCost["Accumulate Token Usage & Cost ($)"]
        TrackCost --> Decision{"Response Type"}

        Decision -- "Final Text (No Tools)" --> NotifyComplete["Emit Done Notification\n• Terminal Bell (BEL)\n• Desktop Push (OSC 777/9/99)"]
        NotifyComplete --> FinalReturn(["Emit Final Output to User"])

        Decision -- "Tool Calls (1..N)" --> ConcurrencyMgr["Parallel Tool Concurrency Manager\n(tokio::join_all bounded by semaphore: max_concurrency=8)"]
    end

    ConcurrencyMgr --> Dispatcher{"Tool Call Dispatcher\n(src/agent_loop.rs)"}

    %% ROUTE 1: In-Process Planning Scratchpad
    subgraph ROUTE1 ["Route 1: Cognitive Scratchpad"]
        Dispatcher -- "name == '_plan'" --> PlanHandler["In-Process Pseudo-Tool (_plan)\n• No shell/subprocess spawned\n• Emits trace event to /dev/tty\n• Returns 'acknowledged'"]
    end

    %% ROUTE 2: Cognitive Sub-Agent Delegation
    subgraph ROUTE2 ["Route 2: Cognitive Sub-Agent Delegation"]
        Dispatcher -- "is_agent && in list_agents()" --> SubAgentCheck{"Depth < max_agent_depth?"}
        SubAgentCheck -- "Yes" --> SubAgentSpawn["Spawn Subprocess: aichat --agent name\n• Isolated Child Process with unique PID\n• Private tools (e.g. researcher, coder)\n• Propagates AICHAT_AGENT_DEPTH+1"]
        SubAgentSpawn --> ChildLoop["Child Agent Loop Runs to Completion"]
        ChildLoop --> SubAgentResult["Return Synthesized Report + Cost to Parent"]
        SubAgentCheck -- "Exceeded Depth" --> DepthErr["Return Depth Limit Exceeded Error"]
    end

    %% ROUTE 3: Deterministic Tools & Output Routing
    subgraph ROUTE3 ["Route 3: Atomic Tools & MCP Execution"]
        Dispatcher -- "Atomic Tool / Subcommand / MCP" --> ToolExec{"Tool Execution Backend"}
        ToolExec -- "MCP Protocol" --> MCPClient["Native Rust In-Process MCP Client"]
        ToolExec -- "Shell / Argc Tool" --> ShellExec["Execute local tool script (bin/tool)"]

        MCPClient --> RawOutput["Raw Tool Result Generated"]
        ShellExec --> RawOutput

        %% Output Routing Sub-pipeline
        subgraph OUTPUT_ROUTING ["Declarative Output Routing Pipeline"]
            RawOutput --> CheckRouting{"Output Routing Rule"}

            %% Pipe Chaining
            CheckRouting -- "output.destination == 'pipe'" --> PipeRoute["Pipe Destination Target\n(e.g., summarize_text.sh)\n• Cycle Detection Protection\n• No LLM round-trip"]
            PipeRoute --> PipeExec["Execute Target Tool with --input"]
            PipeExec --> PipeResult["Pass summary/digest to LLM context"]

            %% File Destination
            CheckRouting -- "output.destination == 'file'" --> FileRoute["File Destination Target\n• Render path: /tmp/name-timestamp.ext\n• Write full content directly to disk"]
            FileRoute --> FileConfirm["Return JSON Receipt to Context\n{ written_to, size_bytes, hint }"]

            %% Auto-Capping / Raw
            CheckRouting -- "Default (Direct)" --> CapCheck{"Size > tool_output_limit?\n(default: 16 KB)"}
            CapCheck -- "Yes (Large Output)" --> AutoCap["Auto-Capping Triggered\n• Write full output to /tmp/aichat-tool-*.out\n• Extract preview snippet"]
            AutoCap --> CapResult["Return JSON Handle to Context\n{ preview, full_output_path, total_bytes, hint }"]

            CapCheck -- "No (Small Output)" --> DirectPass["Return Raw Output directly to Context"]
        end
    end

    %% Rejoining the Loop
    PlanHandler --> CollectResults["Collect & Merge Turn Tool Results"]
    SubAgentResult --> CollectResults
    DepthErr --> CollectResults
    PipeResult --> CollectResults
    FileConfirm --> CollectResults
    CapResult --> CollectResults
    DirectPass --> CollectResults

    CollectResults --> AppendHistory["Append Tool Results to Message History"]
    AppendHistory --> CheckCircuit{"Circuit Breaker Check\n(3 consecutive failures?)"}
    CheckCircuit -- "Tripped" --> CircuitHalt["Halt Loop / Return Partial Results"]
    CircuitHalt --> FinalReturn
    CheckCircuit -- "OK" --> IncTurn["Increment Turn Counter (turn = turn + 1)"]
    IncTurn --> UpdateObs
```

