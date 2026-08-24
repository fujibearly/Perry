# Rust MCP Bridge — Design

## Module Structure

```
src/
├── mcp.rs                    # Self-contained MCP bridge module
├── function.rs               # Modified: routing check in ToolCall::eval()
├── config/
│   └── mod.rs                # Modified: McpServerConfig struct, load/merge MCP tools
└── cli.rs                    # Modified: --sync-mcp flag
```

Single file `src/mcp.rs` — no subdirectory. The module is small enough (~400-600 lines) to not warrant splitting.

## Data Structures

### Configuration (in `src/config/mod.rs`)

```rust
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default = "default_mcp_timeout")]
    pub timeout: u64,  // seconds, per-call
}

fn default_mcp_timeout() -> u64 { 60 }
```

Added to `Config`:
```rust
pub struct Config {
    // ... existing fields ...
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(skip)]
    mcp_tools: IndexMap<String, McpToolEntry>,  // namespaced_name → entry
}
```

### MCP Tool Registry (in `src/mcp.rs`)

```rust
/// A discovered MCP tool, cached to disk and held in memory.
struct McpToolEntry {
    server_name: String,       // which server owns this tool
    original_name: String,     // tool name as declared by the server
    declaration: FunctionDeclaration,  // namespaced, ready to merge
}

/// Cache file format (per server), stored at <config-dir>/mcp-cache/<name>.json
#[derive(Serialize, Deserialize)]
struct McpCacheFile {
    config_hash: String,       // sha256 of command+args+env, for invalidation
    tools: Vec<CachedTool>,
}

#[derive(Serialize, Deserialize)]
struct CachedTool {
    name: String,              // original (un-namespaced) tool name
    description: String,
    parameters: serde_json::Value,  // inputSchema as-is
}

/// A live connection to a running MCP server.
struct McpConnection {
    child: tokio::process::Child,
    stdin: tokio::io::BufWriter<tokio::process::ChildStdin>,
    stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}
```

### JSON-RPC Messages (in `src/mcp.rs`)

```rust
#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,  // always "2.0"
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    method: Option<String>,  // for notifications (no id)
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}
```

## Module Public Interface (`src/mcp.rs`)

```rust
/// Load MCP tool declarations from cache (or discover if cache miss).
/// Called during Config::load_functions().
/// Returns Vec<FunctionDeclaration> ready to merge into Config.functions.
pub fn load_mcp_tools(
    servers: &[McpServerConfig],
    cache_dir: &Path,
) -> Result<IndexMap<String, McpToolEntry>>;

/// Force re-discovery of all servers, updating the cache.
/// Called by --sync-mcp.
pub async fn sync_mcp_tools(
    servers: &[McpServerConfig],
    cache_dir: &Path,
) -> Result<IndexMap<String, McpToolEntry>>;

/// Call an MCP tool by namespaced name. Spawns server on first call.
/// Returns the tool result as a Value (same contract as ToolCall::eval).
pub fn call_mcp_tool(
    server_config: &McpServerConfig,
    tool_name: &str,         // original (un-namespaced) name
    arguments: Value,
    timeout: Duration,
) -> Result<Value>;
```

The module internally holds a `OnceCell<Mutex<HashMap<String, McpConnection>>>` for live connections (lazily populated on first call per server).

## Integration Points

### 1. Tool Discovery — `Config::load_functions()` (config/mod.rs)

```rust
fn load_functions(&mut self) -> Result<()> {
    self.functions = Functions::init(&Self::functions_file())?;

    // NEW: Load MCP tools from cache, merge into declarations
    #[cfg(feature = "mcp")]
    {
        let cache_dir = Self::mcp_cache_dir();
        self.mcp_tools = mcp::load_mcp_tools(&self.mcp_servers, &cache_dir)?;
        let mcp_declarations: Vec<FunctionDeclaration> = self.mcp_tools
            .values()
            .map(|entry| entry.declaration.clone())
            .collect();
        self.functions.extend(mcp_declarations);
    }

    Ok(())
}
```

`Functions` needs a new `extend` method (trivial — push to the internal `Vec<FunctionDeclaration>`).

### 2. Tool Invocation — `ToolCall::eval()` (function.rs)

```rust
pub fn eval(&self, config: &GlobalConfig) -> Result<Value> {
    // NEW: Check if this is an MCP tool first
    #[cfg(feature = "mcp")]
    {
        let config_read = config.read();
        if let Some(entry) = config_read.mcp_tools.get(&self.name) {
            let server_config = config_read.mcp_servers
                .iter()
                .find(|s| s.name == entry.server_name)
                .expect("MCP server config missing for registered tool");
            let timeout = Duration::from_secs(server_config.timeout);
            return mcp::call_mcp_tool(
                server_config,
                &entry.original_name,
                self.arguments.clone(),
                timeout,
            );
        }
    }

    // Existing shell-exec path (unchanged)
    let (call_name, cmd_name, mut cmd_args, envs) = match &config.read().agent {
        Some(agent) => self.extract_call_config_from_agent(config, agent)?,
        None => self.extract_call_config_from_config(config)?,
    };
    // ... rest unchanged ...
}
```

### 3. Tool Filtering — `Config::select_functions()` (config/mod.rs)

MCP server names become valid `use_tools` group names. Modification to the filtering logic:

```rust
// In select_functions(), within the use_tools split loop:
for item in use_tools.split(',') {
    let item = item.trim();
    if let Some(values) = self.mapping_tools.get(item) {
        // existing: expand mapping_tools alias
        tool_names.extend(/* ... */);
    } else if declaration_names.contains(item) {
        // existing: direct tool name match
        tool_names.insert(item.to_string());
    }
    // NEW: server name matches all tools from that MCP server
    #[cfg(feature = "mcp")]
    else {
        let prefix = format!("{item}__");
        tool_names.extend(
            declaration_names.iter()
                .filter(|n| n.starts_with(&prefix))
                .cloned()
        );
    }
}
```

### 4. CLI — `--sync-mcp` (cli.rs, main.rs)

```rust
// In Cli struct:
#[arg(long)]
pub sync_mcp: bool,

// In run():
if cli.sync_mcp {
    #[cfg(feature = "mcp")]
    {
        let servers = config.read().mcp_servers.clone();
        let cache_dir = Config::mcp_cache_dir();
        mcp::sync_mcp_tools(&servers, &cache_dir).await?;
        println!("MCP tools synced.");
    }
    return Ok(());
}
```

### 5. Info Display (config/mod.rs)

In the `info()` method, append MCP server status:

```
MCP Servers:
  filesystem (3 tools) [cached]
  git (5 tools) [cached]
```

Or when running after `--sync-mcp`:

```
MCP Servers:
  filesystem (3 tools) [synced]
  git: error — command not found
```

## Internal Design of `src/mcp.rs`

### Connection Lifecycle

```
                        ┌─────────────┐
         load_mcp_tools │  Read cache │ (no server spawn)
         ─────────────► │  or discover│
                        └──────┬──────┘
                               │ cache miss only
                               ▼
                        ┌─────────────┐
                        │ Spawn server│
                        │ initialize  │
                        │ tools/list  │
                        │ Write cache │
                        │ Shutdown    │ (close stdin, wait)
                        └─────────────┘

                        ┌─────────────┐
          call_mcp_tool │ Spawn server│ (if not already live)
          ─────────────►│ initialize  │
                        │ tools/call  │
                        │ Keep alive  │
                        └─────────────┘
```

Two separate spawn events:
1. **Discovery spawn** (during `load_mcp_tools` on cache miss, or `sync_mcp_tools`): spawn, handshake, `tools/list`, write cache, shutdown. Server doesn't stay alive.
2. **Invocation spawn** (first `call_mcp_tool` for a server): spawn, handshake, then keep alive for subsequent calls. Held in a process-global `Lazy<Mutex<HashMap<String, McpConnection>>>`.

### Async/Sync Bridge

`ToolCall::eval()` is synchronous. The MCP module is async internally. Bridge via:

```rust
pub fn call_mcp_tool(/* ... */) -> Result<Value> {
    // Use the existing tokio runtime (we're always inside one)
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(call_mcp_tool_async(/* ... */))
    })
}
```

`block_in_place` is safe here because aichat always runs on the multi-threaded tokio runtime. This avoids spawning a separate runtime or passing a handle around.

### JSON-RPC over stdio

Communication protocol:
- Write: serialize `JsonRpcRequest` → write as single line (no embedded newlines in JSON) → flush
- Read: read lines from stdout, skip notifications (messages with no `id`), match response `id` to request `id`
- Framing: **newline-delimited JSON** (one JSON object per line). This is the standard MCP stdio transport framing.

```rust
impl McpConnection {
    async fn send_request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let line = serde_json::to_string(&request)?;
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        // Read lines until we get a response matching our id
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).await?;
            if line.is_empty() {
                bail!("MCP server closed connection");
            }
            let response: JsonRpcResponse = serde_json::from_str(&line)?;
            if response.id == Some(id) {
                if let Some(error) = response.error {
                    bail!("MCP error {}: {}", error.code, error.message);
                }
                return Ok(response.result.unwrap_or(Value::Null));
            }
            // else: notification, discard and continue reading
        }
    }
}
```

### Cache Format

File: `<config-dir>/mcp-cache/<server-name>.json`

```json
{
  "config_hash": "a3f2...b7c1",
  "tools": [
    {
      "name": "read_file",
      "description": "Read the contents of a file",
      "parameters": {
        "type": "object",
        "properties": {
          "path": { "type": "string", "description": "File path to read" }
        },
        "required": ["path"]
      }
    }
  ]
}
```

Hash computation: `sha256(format!("{command}\0{args_joined}\0{env_sorted}"))`.

### Conversion: MCP Tool → FunctionDeclaration

```rust
fn mcp_tool_to_declaration(server_name: &str, tool: &CachedTool) -> FunctionDeclaration {
    let namespaced_name = format!("{}__{}", server_name, tool.name);
    let parameters: JsonSchema = serde_json::from_value(tool.parameters.clone())
        .unwrap_or_else(|_| JsonSchema {
            type_value: Some("object".to_string()),
            ..Default::default()
        });
    FunctionDeclaration {
        name: namespaced_name,
        description: tool.description.clone(),
        parameters,
        agent: false,
    }
}
```

### Error Handling

All MCP errors are converted to tool-result errors, never panics:

```rust
fn mcp_error_to_tool_result(err: anyhow::Error) -> Value {
    json!({
        "error": {
            "type": "tool_execution_error",
            "message": format!("MCP: {err}")
        }
    })
}
```

In `call_mcp_tool`, errors are caught and returned as `Ok(mcp_error_to_tool_result(err))` so the LLM can react, rather than `Err(...)` which would abort the tool loop.

### Timeout

```rust
async fn call_mcp_tool_async(/* ... */) -> Result<Value> {
    match tokio::time::timeout(timeout, do_call(/* ... */)).await {
        Ok(result) => result,
        Err(_) => Ok(json!({
            "error": {
                "type": "tool_execution_error",
                "message": "MCP tool call timed out"
            }
        }))
    }
}
```

### Server Shutdown (on exit)

Register an `atexit`-style cleanup. Since aichat uses `tokio::signal` and abort signals, hook into the existing abort signal path:

```rust
pub fn shutdown_all_mcp_servers() {
    // Called from main() cleanup or abort signal handler
    let mut connections = MCP_CONNECTIONS.lock();
    for (name, conn) in connections.drain() {
        // Drop closes stdin, which signals server to exit
        // Then kill if still alive after timeout
        drop(conn.stdin);
        if let Some(mut child) = conn.child {
            let _ = child.start_kill();
        }
    }
}
```

## Cargo Feature Flag

```toml
[features]
default = ["mcp"]
mcp = []
```

No additional crate dependencies needed — `tokio`, `serde_json`, `sha2` are already present. The MCP module uses only what's already in `Cargo.toml`.

## Interaction with Agent-Level MCP Servers

When an agent is loaded (`Agent::init`), its `AgentConfig` may contain additional `mcp_servers`. These are:
1. Loaded from the agent's `config.yaml`
2. Merged with global `mcp_servers` (agent wins on name collision)
3. Their tools are discovered/cached separately under `<config-dir>/mcp-cache/<agent>__<server>.json`
4. Merged into the agent's `Functions` alongside its shell-exec tools

This means `Agent::init` needs a similar `load_mcp_tools` call, but scoped to the agent's servers.

## Sequence Diagrams

### First Run (cache miss)

```
Config::init()
  └─ load_functions()
       ├─ Functions::init()              (shell-exec tools from functions.json)
       └─ mcp::load_mcp_tools()
            ├─ read cache file           → MISS
            ├─ spawn server process
            ├─ send initialize
            ├─ recv initialize response
            ├─ send notifications/initialized
            ├─ send tools/list
            ├─ recv tools list
            ├─ write cache file
            ├─ shutdown server (close stdin)
            └─ return Vec<FunctionDeclaration>
```

### Subsequent Run (cache hit)

```
Config::init()
  └─ load_functions()
       ├─ Functions::init()
       └─ mcp::load_mcp_tools()
            ├─ read cache file           → HIT (hash matches)
            └─ return Vec<FunctionDeclaration> (from cache, no spawn)
```

### Tool Invocation

```
run_directive() loop
  └─ eval_tool_calls()
       └─ ToolCall::eval()
            ├─ check mcp_tools map       → MATCH ("filesystem__read_file")
            └─ mcp::call_mcp_tool()
                 ├─ check live connections → MISS
                 ├─ spawn server process
                 ├─ send initialize
                 ├─ recv initialize response
                 ├─ send notifications/initialized
                 ├─ store in MCP_CONNECTIONS
                 ├─ send tools/call { name: "read_file", arguments: {...} }
                 ├─ recv CallToolResult
                 ├─ parse content blocks → string
                 └─ return Ok(Value)
```

### --sync-mcp

```
main()
  └─ mcp::sync_mcp_tools()
       ├─ for each server in config:
       │    ├─ spawn server
       │    ├─ initialize handshake
       │    ├─ tools/list
       │    ├─ write cache
       │    └─ shutdown
       └─ print summary
```

## Files Modified (summary)

| File | Change |
|------|--------|
| `Cargo.toml` | Add `[features]` section with `mcp` feature |
| `src/mcp.rs` | **NEW** — entire MCP bridge module |
| `src/main.rs` | Add `mod mcp;`, `--sync-mcp` handling, shutdown hook |
| `src/function.rs` | Add MCP routing check at top of `ToolCall::eval()`, add `Functions::extend()` |
| `src/config/mod.rs` | Add `McpServerConfig`, `mcp_servers` field, `mcp_tools` field, modify `load_functions()`, modify `select_functions()`, modify `info()` |
| `src/config/agent.rs` | Add `mcp_servers` to `AgentConfig`, load agent-level MCP tools in `Agent::init()` |
| `src/cli.rs` | Add `--sync-mcp` flag |
| `config.example.yaml` | Add commented `mcp_servers` example |

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| `block_in_place` deadlock if called from single-threaded context | aichat always uses `rt-multi-thread`. Assert or document this. |
| MCP server that writes to stdout without proper framing (e.g., debug prints) | Read loop skips unparseable lines (log at debug level). |
| Server that never responds to initialize | 10-second timeout on handshake (NFR-1.2). |
| Large tool lists bloating LLM context | Same problem exists for shell-exec tools today. Not introduced by this change. |
| Cache staleness (server binary updated but config unchanged) | `--sync-mcp` is the escape hatch. Document it. |
