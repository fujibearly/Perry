# Client-Side Agent Loop Enhancements — Design

## Module Structure

```
src/
├── agent_loop.rs              # NEW: Core loop logic, progress, planning tool
├── function.rs                # Modified: async eval_tool_calls, parallel dispatch
├── main.rs                    # Modified: run_directive calls into agent_loop
├── repl/
│   └── mod.rs                 # Modified: ask_inner calls into agent_loop
├── config/
│   ├── mod.rs                 # Modified: AgentLoopConfig struct, parsing
│   └── agent.rs              # Modified: per-agent max_turns override
└── mcp.rs                     # Modified: remove block_in_place, expose async directly
```

New file: `src/agent_loop.rs` (~400-500 lines). Contains the unified loop logic, progress reporting, planning tool declaration, and sub-agent subprocess spawning. This avoids duplicating the enhanced logic between `main.rs` and `repl/mod.rs`.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Caller (main.rs / repl)                       │
│  run_directive / ask_inner → agent_loop::run(...)                    │
└────────────────────────────────────┬────────────────────────────────┘
                                     │
                    ┌────────────────▼────────────────┐
                    │       agent_loop::run()          │
                    │                                  │
                    │  loop (turn = 1..=max_turns):    │
                    │    1. call LLM (stream or not)   │
                    │    2. extract tool_calls         │
                    │    3. separate _plan from real   │
                    │    4. eval_tool_calls_async()    │
                    │       ├─ parallel dispatch       │
                    │       ├─ sub-agent subprocess    │
                    │       └─ MCP / shell routing     │
                    │    5. merge results into input   │
                    │    6. emit progress events       │
                    │    7. if no tool_calls → break   │
                    └────────────────┬────────────────┘
                                     │
              ┌──────────────────────┼──────────────────────┐
              │                      │                       │
   ┌──────────▼──────────┐  ┌───────▼────────┐  ┌─────────▼──────────┐
   │  Shell-exec tools    │  │  MCP tools     │  │  Sub-agent tools   │
   │  (spawn_blocking)    │  │  (async await) │  │  (aichat subprocess│
   └─────────────────────┘  └────────────────┘  │   own PID/status)  │
                                                  └────────────────────┘
```

### Sub-agent as subprocess (Model A)

Each sub-agent is a separate `aichat` process. This gives every agent:
- Its own PID, status file, OSC title, bell
- Its own turn budget and session
- Process-level isolation (crash doesn't take down parent)
- Native visibility to tmux, Herdr, Agent Deck

The parent blocks until the sub-agent exits (Model A — synchronous delegation). The subprocess boundary enables future evolution to Model B (non-blocking) without architectural changes.

```
Parent aichat (PID 1000)              Sub-agent aichat (PID 1042)
┌─────────────────────────┐           ┌─────────────────────────┐
│ turn 3/20               │           │ turn 1/10               │
│ tool_call: researcher   │──spawn──▶ │ --agent researcher      │
│   (blocks on PID 1042)  │           │ own status file         │
│                         │◀──exit──  │ own OSC title           │
│ tool_result: "found..." │           │ exits with stdout       │
└─────────────────────────┘           └─────────────────────────┘
status: aichat-1000.json              status: aichat-1042.json
```

## Data Structures

### Configuration (in `src/config/mod.rs`)

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentLoopConfig {
    /// Maximum turns before stopping. Default: 20.
    pub max_turns: usize,
    /// Maximum concurrent tool executions. Default: 8.
    pub max_concurrency: usize,
    /// Maximum sub-agent nesting depth. Default: 3.
    pub max_agent_depth: usize,
    /// Print live trace events to stderr. Default: false.
    pub show_trace: bool,
    /// Inject the _plan pseudo-tool. Default: true.
    pub planning_tool: bool,
    /// Emit OSC 0/2 terminal title updates. Default: true.
    pub osc_title: bool,
    /// Maintain a JSON status file for external tools. Default: true.
    pub status_file: bool,
    /// Emit BEL + OSC 777 notifications on completion/blocked. Default: true.
    pub notify: bool,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_turns: 20,
            max_concurrency: 8,
            max_agent_depth: 3,
            show_trace: false,
            planning_tool: true,
            osc_title: true,
            status_file: true,
            notify: true,
        }
    }
}
```

Added to `Config`:
```rust
pub struct Config {
    // ... existing fields ...
    #[serde(default)]
    pub agent_loop: AgentLoopConfig,
}
```

Environment variable overrides applied in `Config::init()`:
```rust
if let Ok(v) = std::env::var("AICHAT_AGENT_LOOP_MAX_TURNS") {
    if let Ok(n) = v.parse::<usize>() { self.agent_loop.max_turns = n; }
}
if let Ok(v) = std::env::var("AICHAT_AGENT_LOOP_SHOW_TRACE") {
    match v.as_str() {
        "true" | "1" => self.agent_loop.show_trace = true,
        "false" | "0" => self.agent_loop.show_trace = false,
        _ => {}
    }
}
```

### Agent Loop Context (in `src/agent_loop.rs`)

```rust
/// Parameters for a single agent loop invocation.
pub struct AgentLoopParams<'a> {
    pub config: &'a GlobalConfig,
    pub abort_signal: AbortSignal,
    pub code_mode: bool,
    /// Progress reporter (shared with caller for rendering).
    pub progress: AgentLoopProgress,
}

/// Return value from a completed agent loop.
pub struct AgentLoopOutput {
    pub usage: TokenUsage,
    pub final_text: String,
}
```

Note: No `depth` field in `AgentLoopParams`. Depth is tracked across process boundaries via `AICHAT_AGENT_DEPTH` env var, read at startup and checked before spawning sub-agents.

### Progress Reporting (in `src/agent_loop.rs`)

```rust
/// Events emitted during the agent loop for observability.
#[derive(Debug, Clone)]
pub enum AgentLoopEvent {
    TurnStart { turn: usize, max_turns: usize },
    ToolStart { name: String, id: Option<String> },
    ToolComplete { name: String, duration: Duration, success: bool },
    SubAgentStart { agent_name: String, pid: u32 },
    SubAgentComplete { agent_name: String, pid: u32, duration: Duration, success: bool },
    PlanReceived { content: String },
    BudgetWarning { turn: usize, max_turns: usize },
    BudgetExhausted { max_turns: usize },
    LoopComplete,
}

/// Thread-safe progress tracker, mirrors OpenAIResponsesProgress pattern.
#[derive(Debug, Clone, Default)]
pub struct AgentLoopProgress {
    state: Arc<Mutex<AgentLoopProgressState>>,
}

#[derive(Debug, Default)]
struct AgentLoopProgressState {
    started_at: Option<Instant>,
    current_turn: usize,
    max_turns: usize,
    active_tools: Vec<String>,
    event_sender: Option<UnboundedSender<AgentLoopEvent>>,
}

impl AgentLoopProgress {
    pub fn live() -> (Self, UnboundedReceiver<AgentLoopEvent>) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let progress = Self::default();
        {
            let mut state = progress.state.lock();
            state.started_at = Some(Instant::now());
            state.event_sender = Some(sender);
        }
        (progress, receiver)
    }

    pub fn emit(&self, event: AgentLoopEvent) {
        let sender = self.state.lock().event_sender.clone();
        if let Some(sender) = sender {
            let _ = sender.send(event);
        }
    }

    pub fn snapshot(&self) -> AgentLoopSnapshot { /* ... */ }
}
```

### Planning Tool Declaration (in `src/agent_loop.rs`)

```rust
/// Returns the FunctionDeclaration for the built-in _plan tool.
pub fn plan_tool_declaration() -> FunctionDeclaration {
    FunctionDeclaration {
        name: "_plan".to_string(),
        description: "Write your reasoning, task decomposition, or plan to a scratchpad. \
            The content will be available in your next turn's context but will not be \
            shown to the user. Use this to think through complex tasks before acting."
            .to_string(),
        parameters: JsonSchema {
            type_value: Some("object".to_string()),
            properties: Some({
                let mut props = IndexMap::new();
                props.insert(
                    "thought".to_string(),
                    JsonSchema {
                        type_value: Some("string".to_string()),
                        description: Some(
                            "Your reasoning, plan, or task decomposition.".to_string(),
                        ),
                        ..Default::default()
                    },
                );
                props
            }),
            required: Some(vec!["thought".to_string()]),
            ..Default::default()
        },
        agent: false,
    }
}
```

## Core Loop Design

### Unified `agent_loop::run()`

The current pattern has two nearly identical recursive functions:
- `run_directive` in `main.rs` (CLI)
- `ask_inner` in `repl/mod.rs` (REPL)

Both will be refactored to call a single `agent_loop::run()` that encapsulates the enhanced loop logic. The callers handle mode-specific concerns (session compression in REPL, code extraction in CLI) before and after the loop.

```rust
/// Run the agent loop: call LLM, execute tools, iterate — up to max_turns.
pub async fn run(input: Input, params: AgentLoopParams<'_>) -> Result<AgentLoopOutput> {
    let max_turns = params.config.read().agent_loop.max_turns;
    let mut current_input = input;
    let mut total_usage = TokenUsage::default();
    let mut last_text = String::new();

    for turn in 1..=max_turns {
        params.progress.emit(AgentLoopEvent::TurnStart { turn, max_turns });

        // 1. Call the LLM (returns raw tool_calls, does not eval them)
        let client = current_input.create_client()?;
        let (output, tool_calls) = call_llm_raw(&current_input, &client, &params).await?;

        params.config.write().after_chat_completion(
            &current_input, &output.text, &[]
        )?;
        total_usage.add(output.usage());
        last_text = output.text.clone();

        if tool_calls.is_empty() {
            // No tools → loop complete
            params.progress.emit(AgentLoopEvent::LoopComplete);
            return Ok(AgentLoopOutput {
                usage: total_usage,
                final_text: output.text,
            });
        }

        // 2. Separate _plan calls from real tool calls
        let (plan_calls, real_calls) = partition_plan_calls(tool_calls);
        handle_plan_calls(&plan_calls, &params);

        // 3. Execute real tool calls (parallel, with sub-agent routing)
        let tool_results = eval_tool_calls_parallel(
            params.config,
            real_calls,
            params.abort_signal.clone(),
            &params.progress,
        ).await?;

        // 4. Merge results + plan content into next input
        current_input = current_input.merge_tool_results(output.text, tool_results);
        if !plan_calls.is_empty() {
            current_input = inject_plan_context(current_input, &plan_calls);
        }

        // 5. Budget warning
        if turn >= max_turns - 2 {
            params.progress.emit(AgentLoopEvent::BudgetWarning { turn, max_turns });
        }
    }

    // Budget exhausted
    params.progress.emit(AgentLoopEvent::BudgetExhausted { max_turns });
    eprintln!(
        "Warning: Agent loop reached the {max_turns}-turn limit. \
         Increase with `agent_loop.max_turns` in config.yaml or \
         AICHAT_AGENT_LOOP_MAX_TURNS env var."
    );

    Ok(AgentLoopOutput {
        usage: total_usage,
        final_text: last_text,
    })
}
```

### Key difference from current code

The current code uses `#[async_recursion]` — each turn is a recursive call that builds up the call stack. The new design uses an iterative loop (`for turn in 1..=max_turns`). This:
- Makes the turn budget trivial to enforce (it's the loop bound)
- Avoids deep stack frames for long agent runs
- Makes progress reporting natural (the turn counter is right there)
- Eliminates `async_recursion` dependency for this path

## Parallel Tool Execution Design

### `eval_tool_calls_parallel()`

```rust
pub async fn eval_tool_calls_parallel(
    config: &GlobalConfig,
    calls: Vec<ToolCall>,
    abort_signal: AbortSignal,
    progress: &AgentLoopProgress,
) -> Result<Vec<ToolResult>> {
    if calls.is_empty() {
        return Ok(vec![]);
    }

    let calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.");
    }

    let max_concurrency = config.read().agent_loop.max_concurrency;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrency));

    let futures: Vec<_> = calls
        .into_iter()
        .map(|call| {
            let config = config.clone();
            let semaphore = semaphore.clone();
            let abort_signal = abort_signal.clone();
            let progress = progress.clone();
            async move {
                let _permit = semaphore.acquire().await.unwrap();
                let start = Instant::now();
                progress.emit(AgentLoopEvent::ToolStart {
                    name: call.name.clone(),
                    id: call.id.clone(),
                });

                let result = eval_single_tool_async(&config, &call, abort_signal).await;
                let duration = start.elapsed();

                let output = match result {
                    Ok(value) => {
                        progress.emit(AgentLoopEvent::ToolComplete {
                            name: call.name.clone(),
                            duration,
                            success: true,
                        });
                        if value.is_null() { json!("DONE") } else { value }
                    }
                    Err(_) => {
                        progress.emit(AgentLoopEvent::ToolComplete {
                            name: call.name.clone(),
                            duration,
                            success: false,
                        });
                        json!({
                            "error": {
                                "type": "tool_execution_error",
                                "message": "The tool call failed. Fix its arguments or choose another tool."
                            }
                        })
                    }
                };
                ToolResult::new(call, output)
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    // Check if all results are "DONE" (null-equivalent) — preserve existing behavior
    let is_all_done = results.iter().all(|r| r.output == json!("DONE"));
    if is_all_done {
        return Ok(vec![]);
    }

    Ok(results)
}
```

### Single Tool Dispatch

```rust
async fn eval_single_tool_async(
    config: &GlobalConfig,
    call: &ToolCall,
    abort_signal: AbortSignal,
) -> Result<Value> {
    // Route 1: MCP tools (async native)
    #[cfg(feature = "mcp")]
    {
        if let Some(entry) = config.read().mcp_tools.get(&call.name) {
            return mcp::call_mcp_tool_async(/* ... */).await;
        }
    }

    // Route 2: Agent tools (subprocess)
    if is_agent_tool(config, &call.name) {
        return eval_agent_tool_subprocess(config, call).await;
    }

    // Route 3: Shell-exec tools (blocking, wrapped in spawn_blocking)
    let config = config.clone();
    let call = call.clone();
    tokio::task::spawn_blocking(move || call.eval_shell(&config))
        .await
        .map_err(|e| anyhow::anyhow!("Tool task panicked: {e}"))?
}
```

## Sub-Agent Delegation Design (Subprocess Model)

### `eval_agent_tool_subprocess()`

```rust
async fn eval_agent_tool_subprocess(
    config: &GlobalConfig,
    call: &ToolCall,
) -> Result<Value> {
    // 1. Check depth limit
    let current_depth = std::env::var("AICHAT_AGENT_DEPTH")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let max_depth = config.read().agent_loop.max_agent_depth;
    if current_depth >= max_depth {
        bail!(
            "Sub-agent nesting depth {} would exceed maximum {}. \
             Increase with `agent_loop.max_agent_depth` in config.",
            current_depth + 1, max_depth
        );
    }

    // 2. Resolve agent name and build command
    let agent_name = extract_agent_name(config, &call.name)?;
    let task_message = format_agent_arguments(&call.arguments);
    let aichat_bin = std::env::current_exe()?;

    // 3. Build the subprocess command
    let mut cmd = tokio::process::Command::new(&aichat_bin);
    cmd.arg("--agent").arg(&agent_name);
    cmd.arg(&task_message);

    // Pass depth to child
    cmd.env("AICHAT_AGENT_DEPTH", (current_depth + 1).to_string());

    // Inherit config dir so sub-agent sees same agents/tools/MCP
    if let Ok(config_dir) = std::env::var("AICHAT_CONFIG_DIR") {
        cmd.env("AICHAT_CONFIG_DIR", config_dir);
    }

    // Capture stdout (the agent's final output), let stderr pass through for logging
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // 4. Spawn and wait
    let child = cmd.spawn()?;
    let output = child.wait_with_output().await?;

    // 5. Return result
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(json!({"output": text}))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!("Sub-agent '{}' exited with code {:?}", agent_name, output.status.code())
        } else {
            stderr
        };
        Ok(json!({
            "error": {
                "type": "agent_error",
                "message": message
            }
        }))
    }
}
```

### Why subprocess over in-process

| Concern | In-process (rejected) | Subprocess (chosen) |
|---------|----------------------|---------------------|
| Observability | Nested tree, single status file, conflicts | Each process owns its own signals |
| Crash isolation | Sub-agent panic kills parent | Process boundary protects parent |
| Shared state contention | `GlobalConfig` lock shared across all agents | No shared state |
| External tool visibility | Invisible to Herdr/Agent Deck | Each process is detectable |
| Model B evolution | Requires architectural rework | Change "wait" to "spawn in background" |
| Latency | ~0ms (function call) | ~50ms (process spawn) |
| Complexity | Event forwarding, depth tracking, tree rendering | Simple Command::new + wait_with_output |

The ~50ms process spawn overhead is negligible compared to LLM call latency (typically 1-10 seconds per turn).

### Agent name resolution

When the parent's LLM calls a tool with `agent: true`, the name resolution follows the existing pattern:
- The function name in the tool call maps to an agent directory name
- The subprocess is invoked with `--agent <name>`, which triggers `Agent::init()` in the child process
- The child process independently loads the agent's functions, model, instructions, and MCP servers

### Depth tracking via environment variable

```
Parent (depth 0):
  AICHAT_AGENT_DEPTH not set → defaults to 0
  Spawns sub-agent with AICHAT_AGENT_DEPTH=1

Sub-agent (depth 1):
  Reads AICHAT_AGENT_DEPTH=1
  Spawns sub-sub-agent with AICHAT_AGENT_DEPTH=2

Sub-sub-agent (depth 2):
  Reads AICHAT_AGENT_DEPTH=2
  max_agent_depth=3, so 2 < 3 → allowed
  Spawns next with AICHAT_AGENT_DEPTH=3

Sub-sub-sub-agent (depth 3):
  Reads AICHAT_AGENT_DEPTH=3
  max_agent_depth=3, so 3 >= 3 → DENIED, returns error
```

## Planning Tool Design

### Integration points

1. **Injection:** When `agent_loop.planning_tool` is true and the input has tools configured, `_plan` is added to the function declarations sent to the LLM.

2. **Partitioning:** After receiving tool_calls from the LLM, the loop separates `_plan` calls from real tool calls:
   ```rust
   fn partition_plan_calls(calls: Vec<ToolCall>) -> (Vec<ToolCall>, Vec<ToolCall>) {
       calls.into_iter().partition(|c| c.name == "_plan")
   }
   ```

3. **Context injection:** Plan content is merged into the next turn's context as a tool result with `output: "acknowledged"`. The LLM protocol requires every tool call to have a result. The plan text is available in conversation context for the next turn via the tool result message. It is not rendered to the user (tool results are internal context, not user output).

4. **Rendering:** Plan content appears only in trace output (`show_trace: true`) and debug logs. Never in the user-facing stream.

## Caller Refactoring

### `main.rs` — `run_directive`

Before (recursive):
```rust
#[async_recursion::async_recursion]
async fn run_directive(config, input, code_mode, abort_signal) -> Result<TokenUsage> {
    let client = input.create_client()?;
    let (output, tool_results) = call_llm(...).await?;
    if !tool_results.is_empty() {
        let next = run_directive(config, input.merge(...), ...).await?;
        usage.add(next);
    }
    Ok(usage)
}
```

After (delegates to agent_loop):
```rust
async fn run_directive(config, input, code_mode, abort_signal) -> Result<TokenUsage> {
    let has_tools = input.role().functions().map_or(false, |f| !f.is_empty());
    if !has_tools {
        // Simple single-turn (no tools configured) — skip loop overhead
        return run_single_turn(config, input, code_mode, abort_signal).await;
    }
    let (progress, event_rx) = AgentLoopProgress::live();
    let params = AgentLoopParams {
        config: &config, abort_signal, code_mode, progress,
    };
    // Spawn trace/observability renderer (same pattern as run_multi_agent_directive)
    let output = agent_loop::run(input, params).await?;
    Ok(output.usage)
}
```

### `repl/mod.rs` — `ask_inner`

Same pattern: delegate to `agent_loop::run()` when tools are present. Handle session compression before/after.

## MCP Bridge Update

The current `call_mcp_tool` uses `tokio::task::block_in_place` because `eval_tool_calls` is synchronous. With async eval, the bridge simplifies:

```rust
// Before (src/mcp.rs):
pub fn call_mcp_tool(server: &McpServerConfig, name: &str, args: Value, timeout: Duration) -> Result<Value> {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(call_mcp_tool_inner(...))
    })
}

// After:
pub async fn call_mcp_tool_async(server: &McpServerConfig, name: &str, args: Value, timeout: Duration) -> Result<Value> {
    call_mcp_tool_inner(server, name, args, timeout).await
}
```

The synchronous `call_mcp_tool` is kept (deprecated) for any remaining sync callers, but the agent loop uses the async path directly.

## Progress Rendering

The rendering loop follows the proven pattern from `run_multi_agent_directive`:

```rust
let live_run = async {
    tokio::pin!(loop_future);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            result = &mut loop_future => break result,
            Some(event) = event_rx.recv() => render_event(event, &agent_loop_config)?,
            _ = heartbeat.tick() => update_spinner(progress.snapshot())?,
        }
    }
};
```

Trace output format (when `show_trace: true`):
```
Agent loop trace:
  [turn 1/20] calling: execute_command, fs_cat (parallel)
  [turn 1/20] execute_command completed (1.2s)
  [turn 1/20] fs_cat completed (0.1s)
  [turn 2/20] calling: fs_write
  [turn 2/20] fs_write completed (0.3s)
  [turn 3/20] plan: "Need to verify the output by running tests"
  [turn 3/20] calling: execute_command
  [turn 4/20] calling: researcher (sub-agent, PID 1042)
  [turn 4/20] researcher completed (12.3s, PID 1042)
  ...
```

Spinner message format:
```
Turn 3/20 | executing: fs_write (0.3s)
```

## External Observability Design

Each aichat process (including sub-agents) independently emits observability signals. No coordination between processes is needed — each owns its own channels.

### OSC Terminal Title (FR-5.7)

```rust
fn update_terminal_title(state: &str) {
    if *IS_STDOUT_TERMINAL {
        eprint!("\x1b]0;{state}\x07");
    }
}
```

Called on every state transition. Written to stderr to avoid polluting piped output. tmux reads it automatically with `set -g set-titles on`. Herdr's screen manifest can match on it.

Sub-agent processes typically have stdout piped (captured by parent), so `IS_STDOUT_TERMINAL` is false and they don't emit OSC titles. In Model B (future), sub-agents in their own tmux panes would emit their own titles.

### Status File (FR-5.8)

```rust
#[derive(Serialize)]
struct AgentLoopStatus {
    pid: u32,
    state: &'static str, // "idle", "working", "waiting_for_input", "done"
    turn: usize,
    max_turns: usize,
    active_tools: Vec<String>,
    model: String,
    agent: Option<String>,  // agent name, if running as --agent
    depth: usize,           // from AICHAT_AGENT_DEPTH
    elapsed_s: f64,
    session: Option<String>,
    updated_at: String,     // ISO 8601
}
```

Each process writes to `$XDG_RUNTIME_DIR/aichat-<pid>.json`. External tools enumerate all matching files for a fleet view:

```bash
# List all running aichat agents and their states
cat /run/user/1000/aichat-*.json | jq '{pid, state, agent, turn, max_turns}'
```

Sub-agent processes write their own status files (they have their own PIDs). This means during execution, you might see:
```
/run/user/1000/aichat-1000.json  → parent, state: "working", active_tools: ["researcher"]
/run/user/1000/aichat-1042.json  → sub-agent, state: "working", agent: "researcher", depth: 1
```

Both files cleaned up on respective process exit.

### Terminal Notifications (FR-5.9)

```rust
fn notify_terminal(title: &str, message: &str) {
    if !*IS_STDOUT_TERMINAL {
        return;
    }
    eprint!("\x07");
    eprint!("\x1b]777;notify;{title};{message}\x07");
}
```

Only fires when stdout is a tty. Sub-agents with piped stdout naturally don't notify. The parent notifies on its own completion — which implicitly means all sub-agents have already finished (since Model A is blocking).

### Integration with AgentLoopProgress

All observability channels subscribe to the same event stream:

```rust
fn render_event(event: AgentLoopEvent, config: &AgentLoopConfig) -> Result<()> {
    // Trace output
    if config.show_trace { render_trace_line(&event)?; }

    // OSC title
    if config.osc_title { update_terminal_title(&format_title(&event)); }

    // Status file
    if config.status_file { write_status_file(&build_status(&event)); }

    // Notification (only on terminal events)
    if config.notify {
        if let Some((title, msg)) = notification_for_event(&event) {
            notify_terminal(title, msg);
        }
    }

    Ok(())
}
```

### tmux-specific considerations

Since we commit to running inside tmux:

1. **OSC passthrough:** tmux >= 3.3 passes OSC sequences to the outer terminal with `set -g allow-passthrough on`. For desktop notifications to reach the actual terminal emulator, this must be enabled. Without it, tmux consumes the OSC and the bell still works (tmux handles it natively).

2. **Pane title vs window title:** `set -g set-titles on` + `set -g set-titles-string "#{pane_title}"` makes tmux propagate our OSC 0 title to the outer terminal's window title.

3. **Bell behavior:** tmux's `monitor-bell` is per-window. It highlights the window's status-bar entry when a bell fires in a non-focused pane. This is the primary "attention needed" signal.

## Backward Compatibility

1. **No tools configured:** When a role/session has no functions, `run_directive`/`ask_inner` bypass the agent loop entirely and do a single LLM call. Zero overhead added.

2. **Single tool call, no plan:** The loop runs one turn with one concurrent task (semaphore permits 8, but only 1 is used). Functionally identical to today but iterative instead of recursive.

3. **Config absent:** All `AgentLoopConfig` fields have defaults via `#[serde(default)]`. Existing configs without `agent_loop` section work unchanged.

4. **`eval_tool_calls` sync API:** A synchronous wrapper is retained for any code paths outside the agent loop that still call it (e.g., the OpenAI Responses module's `execute_function_calls` which has its own loop). The sync wrapper calls the async version via `block_in_place`.

5. **Sub-agent as subprocess:** The existing `agent: true` field on `FunctionDeclaration` already exists in the codebase. Today it routes to shell-exec with the agent name as a command prefix. The new behavior (spawn aichat subprocess) replaces this. Any existing agent definitions continue to work — same field, better execution model.

## Testing Strategy

1. **Unit tests in `agent_loop.rs`:**
   - Turn budget enforcement (mock LLM that always returns tool_calls → stops at max_turns)
   - Parallel execution ordering (3 tools with varying delays → results in original order)
   - Planning tool partitioning (mix of _plan + real calls → correct split)

2. **Integration tests:**
   - End-to-end with mock client returning multiple tool_calls
   - Sub-agent subprocess: create a test agent, invoke it as a sub-agent, verify output captured
   - Depth enforcement: set AICHAT_AGENT_DEPTH near max, verify sub-agent spawn refused
   - Config parsing: agent_loop section with all fields, partial fields, absent section
   - Environment variable overrides
   - Status file: verify created on start, updated on events, deleted on exit

3. **Existing test preservation:**
   - The sync `eval_tool_calls` wrapper ensures all 298 existing tests pass without modification
   - Wire tests in `src/client/wire_tests.rs` are unaffected (they test client parsing, not the loop)
