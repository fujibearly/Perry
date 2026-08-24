# Rust MCP Bridge — Implementation Tasks

## Task 1: Cargo feature flag and module skeleton

**Files:** `Cargo.toml`, `src/mcp.rs`, `src/main.rs`

1. Add `[features]` section to `Cargo.toml`:
   ```toml
   [features]
   default = ["mcp"]
   mcp = []
   ```
2. Create `src/mcp.rs` with module doc comment, `#![cfg(feature = "mcp")]` guard, and placeholder public functions:
   ```rust
   pub fn load_mcp_tools(...) -> Result<...> { todo!() }
   pub async fn sync_mcp_tools(...) -> Result<...> { todo!() }
   pub fn call_mcp_tool(...) -> Result<Value> { todo!() }
   ```
3. Add `#[cfg(feature = "mcp")] mod mcp;` to `src/main.rs`.
4. Verify: `cargo build` succeeds with and without `--features mcp`.

---

## Task 2: Configuration data structures

**Files:** `src/config/mod.rs`, `config.example.yaml`

1. Add `McpServerConfig` struct:
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
       pub timeout: u64,
   }
   fn default_mcp_timeout() -> u64 { 60 }
   ```
2. Add to `Config` struct:
   ```rust
   #[serde(default)]
   pub mcp_servers: Vec<McpServerConfig>,
   ```
3. Add `mcp_servers` example (commented out) to `config.example.yaml`.
4. Add `Config::mcp_cache_dir()` returning `<config-dir>/mcp-cache/`.
5. Verify: `cargo build`, existing config loads without error, new field defaults to empty vec.

---

## Task 3: JSON-RPC transport layer

**Files:** `src/mcp.rs`

1. Implement `JsonRpcRequest` and `JsonRpcResponse` structs (serialize/deserialize).
2. Implement `McpConnection` struct:
   - `spawn(config: &McpServerConfig) -> Result<Self>` — spawns child process with stdin/stdout pipes, stderr piped to a background reader that logs at debug level.
   - `send_request(&mut self, method: &str, params: Option<Value>) -> Result<Value>` — write newline-delimited JSON, read response lines, skip notifications, match by id.
   - `shutdown(self)` — drop stdin, wait with timeout, kill if needed.
3. Implement initialize handshake:
   - `initialize(&mut self) -> Result<()>` — send `initialize` request (with client info), receive response, send `notifications/initialized`.
4. Verify: Unit test with a mock MCP server script (write a trivial bash/python script that speaks JSON-RPC over stdio, run it from a test).

---

## Task 4: Tool discovery and cache

**Files:** `src/mcp.rs`

1. Define `McpCacheFile` and `CachedTool` structs.
2. Implement `config_hash(config: &McpServerConfig) -> String` — sha256 of command+args+env.
3. Implement cache read:
   - `read_cache(cache_dir: &Path, server_name: &str, expected_hash: &str) -> Option<Vec<CachedTool>>` — read file, check hash, return tools or None.
4. Implement cache write:
   - `write_cache(cache_dir: &Path, server_name: &str, hash: &str, tools: &[CachedTool]) -> Result<()>`
5. Implement discovery:
   - `discover_tools(config: &McpServerConfig) -> Result<Vec<CachedTool>>` — spawn, initialize, `tools/list`, shutdown, return tools.
6. Implement `McpToolEntry` struct and conversion function `mcp_tool_to_declaration()`.
7. Implement public `load_mcp_tools()`:
   - For each non-disabled server: try cache, on miss: discover and cache.
   - Return `IndexMap<String, McpToolEntry>` keyed by namespaced tool name.
8. Implement public `sync_mcp_tools()`:
   - For each non-disabled server: always discover, always write cache.
9. Verify: Integration test — configure a test MCP server, call `load_mcp_tools`, check returned declarations. Call again, verify cache is hit (no spawn).

---

## Task 5: Tool invocation

**Files:** `src/mcp.rs`

1. Implement global connection pool: `static MCP_CONNECTIONS: Lazy<Mutex<HashMap<String, McpConnection>>>`.
2. Implement `call_mcp_tool_async()`:
   - Check pool for existing connection. If absent, spawn + initialize + store.
   - Send `tools/call` with `{ name, arguments }`.
   - Parse `CallToolResult`: concatenate text content blocks, handle image blocks, handle `isError`.
   - Apply timeout wrapper.
3. Implement public `call_mcp_tool()`:
   - `tokio::task::block_in_place` + `Handle::current().block_on(call_mcp_tool_async(...))`.
   - On error, return `Ok(mcp_error_to_tool_result(err))` instead of propagating.
4. Implement `shutdown_all_mcp_servers()`:
   - Drain the pool, drop stdin, kill with timeout.
5. Verify: Integration test — spawn a test MCP server, call a tool, verify result. Call again, verify same connection reused.

---

## Task 6: Integration — tool discovery in Config

**Files:** `src/function.rs`, `src/config/mod.rs`

1. Add `Functions::extend(&mut self, declarations: Vec<FunctionDeclaration>)` method.
2. Add `mcp_tools: IndexMap<String, McpToolEntry>` field to `Config` (skip serialization).
3. Modify `Config::load_functions()`:
   ```rust
   #[cfg(feature = "mcp")]
   {
       let cache_dir = Self::mcp_cache_dir();
       self.mcp_tools = mcp::load_mcp_tools(&self.mcp_servers, &cache_dir)?;
       let declarations = self.mcp_tools.values().map(|e| e.declaration.clone()).collect();
       self.functions.extend(declarations);
   }
   ```
4. Verify: Configure an MCP server in config.yaml, run `cargo run -- --info`, see MCP tools listed.

---

## Task 7: Integration — tool invocation routing

**Files:** `src/function.rs`

1. Modify `ToolCall::eval()` — add MCP check before existing shell-exec path:
   ```rust
   #[cfg(feature = "mcp")]
   {
       let config_read = config.read();
       if let Some(entry) = config_read.mcp_tools.get(&self.name) {
           let server_config = config_read.mcp_servers
               .iter()
               .find(|s| s.name == entry.server_name)
               .unwrap();
           let timeout = Duration::from_secs(server_config.timeout);
           let arguments = /* parse arguments same as existing code */;
           return mcp::call_mcp_tool(server_config, &entry.original_name, arguments, timeout);
       }
   }
   ```
2. Verify: End-to-end test — configure MCP server with a tool, prompt LLM to use it, verify tool is called and result returned.

---

## Task 8: Integration — use_tools filtering

**Files:** `src/config/mod.rs`

1. Modify `select_functions()` — in the `use_tools` split loop, add server-name-as-group matching:
   ```rust
   #[cfg(feature = "mcp")]
   {
       let prefix = format!("{item}__");
       tool_names.extend(
           declaration_names.iter()
               .filter(|n| n.starts_with(&prefix))
               .cloned()
       );
   }
   ```
2. Add MCP server names to the completions list (for REPL `.use-tools` autocomplete).
3. Verify: Configure agent with `use_tools: "filesystem"`, confirm only `filesystem__*` MCP tools are included.

---

## Task 9: CLI --sync-mcp flag

**Files:** `src/cli.rs`, `src/main.rs`

1. Add `--sync-mcp` flag to `Cli` struct.
2. Add handling in `run()`:
   ```rust
   if cli.sync_mcp {
       #[cfg(feature = "mcp")]
       {
           let servers = config.read().mcp_servers.clone();
           let cache_dir = Config::mcp_cache_dir();
           mcp::sync_mcp_tools(&servers, &cache_dir).await?;
       }
       return Ok(());
   }
   ```
3. Print summary: server name, tool count, or error per server.
4. Verify: Run `cargo run -- --sync-mcp` with configured servers, see cache files written.

---

## Task 10: Info display and shutdown hook

**Files:** `src/config/mod.rs`, `src/main.rs`

1. Modify `Config::info()` to append MCP server info:
   ```
   MCP Servers:
     filesystem (3 tools) [cached]
     git (5 tools) [cached]
   ```
2. Add `mcp::shutdown_all_mcp_servers()` call to main exit path (after `run()` returns and in abort signal handler).
3. Verify: `aichat --info` shows MCP section. Ctrl+C during a tool call doesn't leave orphan processes.

---

## Task 11: Agent-level MCP servers

**Files:** `src/config/agent.rs`

1. Add `mcp_servers: Vec<McpServerConfig>` to `AgentConfig` (serde default).
2. In `Agent::init()`, after loading agent functions:
   - Merge global + agent MCP servers (agent wins on name collision).
   - Call `load_mcp_tools()` for the merged set.
   - Extend agent's `Functions` with MCP declarations.
3. Store `mcp_tools` on the `Agent` struct for routing in `ToolCall::extract_call_config_from_agent()`.
4. Verify: Create an agent with agent-level MCP servers, invoke a tool, confirm it routes correctly.

---

## Task 12: Environment variable expansion

**Files:** `src/mcp.rs`

1. Implement `expand_env_vars(s: &str) -> String` — expand `$VAR` and `${VAR}` patterns using `std::env::var`.
2. Apply expansion to `command`, each item in `args`, and each value in `env` before spawning.
3. Verify: Configure `command: "${HOME}/.local/bin/mcp-server"`, confirm it resolves correctly.

---

## Task 13: Documentation and example config

**Files:** `config.example.yaml`, `README.md` (if appropriate)

1. Add documented `mcp_servers` section to `config.example.yaml`.
2. Add `--sync-mcp` to the CLI help text (automatic via clap derive, but verify).
3. Verify: `cargo run -- --help` shows `--sync-mcp`. Example config is valid YAML.

---

## Execution Order

Tasks are ordered by dependency:

```
T1 (skeleton) → T2 (config) → T3 (transport) → T4 (discovery/cache) → T5 (invocation)
                                                                              │
T6 (config integration) ←────────────────────────────────────────────────────┘
T7 (invocation routing) ← T6
T8 (use_tools filtering) ← T6
T9 (--sync-mcp CLI) ← T4
T10 (info + shutdown) ← T5, T6
T11 (agent-level) ← T6, T7
T12 (env expansion) ← T3 (can be done anytime after T3)
T13 (docs) ← all
```

Parallelizable: T8, T9, T12 can be done concurrently once T6 is complete.
