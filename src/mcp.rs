//! Rust MCP Bridge — native MCP client embedded in aichat.
//!
//! Speaks JSON-RPC 2.0 over stdio to MCP server subprocesses.
//! MCP-sourced tools appear as standard `FunctionDeclaration` entries,
//! indistinguishable from shell-exec tools to the rest of the codebase.

use crate::function::FunctionDeclaration;

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

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

fn default_mcp_timeout() -> u64 {
    60
}

// ---------------------------------------------------------------------------
// Environment variable expansion
// ---------------------------------------------------------------------------

/// Expand `$VAR` and `${VAR}` patterns in a string using the process environment.
fn expand_env_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            // Check for ${VAR} or $VAR
            if chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let mut var_name = String::new();
                for ch in chars.by_ref() {
                    if ch == '}' {
                        break;
                    }
                    var_name.push(ch);
                }
                if let Ok(val) = std::env::var(&var_name) {
                    result.push_str(&val);
                }
            } else {
                // $VAR — collect alphanumeric + underscore
                let mut var_name = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        var_name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if var_name.is_empty() {
                    result.push('$');
                } else if let Ok(val) = std::env::var(&var_name) {
                    result.push_str(&val);
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// JSON-RPC message types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
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
    #[allow(dead_code)]
    method: Option<String>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// ---------------------------------------------------------------------------
// MCP Connection — manages a single MCP server subprocess
// ---------------------------------------------------------------------------

/// A live stdio connection to a running MCP server process.
pub struct McpConnection {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpConnection {
    /// Spawn an MCP server subprocess and return the connection (not yet initialized).
    pub fn spawn(config: &McpServerConfig) -> Result<Self> {
        let command = expand_env_vars(&config.command);
        let args: Vec<String> = config.args.iter().map(|a| expand_env_vars(a)).collect();
        let env: HashMap<String, String> = config
            .env
            .iter()
            .map(|(k, v)| (k.clone(), expand_env_vars(v)))
            .collect();

        let mut cmd = tokio::process::Command::new(&command);
        cmd.args(&args);
        cmd.envs(&env);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        // Prevent the child from inheriting signals (e.g., Ctrl+C goes to aichat, not server)
        #[cfg(unix)]
        {
            cmd.process_group(0);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server '{}'", config.name))?;

        let child_stdin = child
            .stdin
            .take()
            .context("Failed to open stdin for MCP server")?;
        let child_stdout = child
            .stdout
            .take()
            .context("Failed to open stdout for MCP server")?;

        // Spawn a background task to drain stderr and log it
        if let Some(stderr) = child.stderr.take() {
            let server_name = config.name.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("[mcp:{}] {}", server_name, line);
                }
            });
        }

        Ok(Self {
            child,
            stdin: BufWriter::new(child_stdin),
            stdout: BufReader::new(child_stdout),
            next_id: 1,
        })
    }

    /// Perform the MCP initialize handshake.
    /// Sends `initialize` request, waits for response, sends `notifications/initialized`.
    pub async fn initialize(&mut self) -> Result<Value> {
        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "aichat",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let result = self
            .send_request("initialize", Some(params))
            .await
            .context("MCP initialize handshake failed")?;

        // Send notifications/initialized (no id, it's a notification)
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        let line = serde_json::to_string(&notification)?;
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        Ok(result)
    }

    /// Send a JSON-RPC request and wait for the matching response.
    /// Notifications (messages without an id) are discarded.
    pub async fn send_request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
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
            let bytes_read = self.stdout.read_line(&mut line).await?;
            if bytes_read == 0 {
                bail!("MCP server closed connection unexpectedly");
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Try to parse as JSON-RPC response
            let response: JsonRpcResponse = match serde_json::from_str(line) {
                Ok(r) => r,
                Err(e) => {
                    // Not valid JSON-RPC — log and skip (server debug output, etc.)
                    debug!("[mcp] Skipping unparseable line: {} ({})", line, e);
                    continue;
                }
            };

            // Notifications have no id — discard them
            match response.id {
                Some(resp_id) if resp_id == id => {
                    if let Some(error) = response.error {
                        bail!("MCP error {}: {}", error.code, error.message);
                    }
                    return Ok(response.result.unwrap_or(Value::Null));
                }
                Some(_other_id) => {
                    // Response for a different request id — shouldn't happen in serial usage,
                    // but skip gracefully
                    continue;
                }
                None => {
                    // Notification — discard
                    continue;
                }
            }
        }
    }

    /// Gracefully shut down the server: close stdin, wait briefly, then kill.
    pub async fn shutdown(mut self) {
        // Close stdin — signals the server to exit
        drop(self.stdin);

        // Give it 5 seconds to exit gracefully
        match tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
            Ok(_) => {}
            Err(_) => {
                // Timed out — force kill
                let _ = self.child.kill().await;
            }
        }
    }

    /// Check if the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

// ---------------------------------------------------------------------------
// Tool discovery and cache
// ---------------------------------------------------------------------------

use crate::function::JsonSchema;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

/// Cache file format, stored at `<cache_dir>/<server_name>.json`.
#[derive(Debug, Serialize, Deserialize)]
struct McpCacheFile {
    config_hash: String,
    tools: Vec<CachedTool>,
}

/// A single tool as stored in the cache file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedTool {
    name: String,
    description: String,
    parameters: Value,
}

/// A discovered MCP tool, cached to disk and held in memory.
#[derive(Debug, Clone)]
pub struct McpToolEntry {
    pub server_name: String,
    pub original_name: String,
    pub declaration: FunctionDeclaration,
}

/// Compute a hash of the server config for cache invalidation.
fn config_hash(config: &McpServerConfig) -> String {
    let mut hasher = Sha256::new();
    hasher.update(config.command.as_bytes());
    hasher.update(b"\0");
    for arg in &config.args {
        hasher.update(arg.as_bytes());
        hasher.update(b"\0");
    }
    // Sort env keys for determinism
    let mut env_pairs: Vec<_> = config.env.iter().collect();
    env_pairs.sort_by_key(|(k, _)| *k);
    for (k, v) in env_pairs {
        hasher.update(k.as_bytes());
        hasher.update(b"=");
        hasher.update(v.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

/// Get the cache file path for a server.
fn cache_file_path(cache_dir: &Path, server_name: &str) -> PathBuf {
    cache_dir.join(format!("{server_name}.json"))
}

/// Try to read tools from the cache. Returns None if cache is missing or invalid.
fn read_cache(cache_dir: &Path, server_name: &str, expected_hash: &str) -> Option<Vec<CachedTool>> {
    let path = cache_file_path(cache_dir, server_name);
    let content = fs::read_to_string(&path).ok()?;
    let cache: McpCacheFile = serde_json::from_str(&content).ok()?;
    if cache.config_hash != expected_hash {
        // Config changed — cache is stale
        None
    } else {
        Some(cache.tools)
    }
}

/// Write tools to the cache file.
fn write_cache(
    cache_dir: &Path,
    server_name: &str,
    hash: &str,
    tools: &[CachedTool],
) -> Result<()> {
    fs::create_dir_all(cache_dir)
        .with_context(|| format!("Failed to create MCP cache dir: {}", cache_dir.display()))?;
    let cache = McpCacheFile {
        config_hash: hash.to_string(),
        tools: tools.to_vec(),
    };
    let content = serde_json::to_string_pretty(&cache)?;
    let path = cache_file_path(cache_dir, server_name);
    fs::write(&path, content)
        .with_context(|| format!("Failed to write MCP cache: {}", path.display()))?;
    Ok(())
}

/// Spawn a server, perform handshake, fetch tools/list, shutdown.
/// Returns the raw tool list from the server.
async fn discover_tools(config: &McpServerConfig) -> Result<Vec<CachedTool>> {
    let mut conn = McpConnection::spawn(config)?;

    let init_timeout = Duration::from_secs(10);
    let init_result = tokio::time::timeout(init_timeout, conn.initialize()).await;
    match init_result {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            conn.shutdown().await;
            return Err(e.context(format!(
                "MCP server '{}' initialize failed",
                config.name
            )));
        }
        Err(_) => {
            conn.shutdown().await;
            bail!(
                "MCP server '{}' initialize timed out ({}s)",
                config.name,
                init_timeout.as_secs()
            );
        }
    }

    let list_result = conn.send_request("tools/list", None).await;
    conn.shutdown().await;

    let result = list_result.with_context(|| {
        format!("MCP server '{}' tools/list request failed", config.name)
    })?;

    let tools_value = result
        .get("tools")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));

    let tools_array = tools_value
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut tools = Vec::with_capacity(tools_array.len());
    for tool in tools_array {
        let name = tool
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = tool
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let parameters = tool
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object"}));

        if !name.is_empty() {
            tools.push(CachedTool {
                name,
                description,
                parameters,
            });
        }
    }

    Ok(tools)
}

/// Convert a CachedTool into a namespaced FunctionDeclaration + McpToolEntry.
fn cached_tool_to_entry(server_name: &str, tool: &CachedTool) -> McpToolEntry {
    let namespaced_name = format!("{}__{}", server_name, tool.name);
    let parameters: JsonSchema = serde_json::from_value(tool.parameters.clone())
        .unwrap_or_else(|_| JsonSchema {
            type_value: Some("object".to_string()),
            description: None,
            properties: None,
            items: None,
            any_of: None,
            enum_value: None,
            default: None,
            required: None,
        });

    McpToolEntry {
        server_name: server_name.to_string(),
        original_name: tool.name.clone(),
        declaration: FunctionDeclaration {
            name: namespaced_name,
            description: tool.description.clone(),
            parameters,
            agent: false,
            output: None,
        },
    }
}

/// Build the tool registry from a list of servers, using cache where available.
/// On cache miss, performs inline discovery (spawns server, fetches tools, caches).
fn build_tool_registry(
    servers: &[McpServerConfig],
    cache_dir: &Path,
    force_discover: bool,
) -> Result<IndexMap<String, McpToolEntry>> {
    let rt = tokio::runtime::Handle::current();
    let mut registry = IndexMap::new();

    for server in servers {
        if server.disabled {
            continue;
        }

        let hash = config_hash(server);

        let tools = if !force_discover {
            read_cache(cache_dir, &server.name, &hash)
        } else {
            None
        };

        let tools = match tools {
            Some(cached) => cached,
            None => {
                // Cache miss — discover inline
                let discovered = tokio::task::block_in_place(|| {
                    rt.block_on(discover_tools(server))
                });
                match discovered {
                    Ok(tools) => {
                        // Write to cache (best-effort)
                        if let Err(e) = write_cache(cache_dir, &server.name, &hash, &tools) {
                            warn!("Failed to write MCP cache for '{}': {}", server.name, e);
                        }
                        tools
                    }
                    Err(e) => {
                        warn!("MCP server '{}' discovery failed: {}", server.name, e);
                        continue;
                    }
                }
            }
        };

        for tool in &tools {
            let entry = cached_tool_to_entry(&server.name, tool);
            registry.insert(entry.declaration.name.clone(), entry);
        }
    }

    Ok(registry)
}

/// Load MCP tool declarations from cache (or discover on cache miss).
/// Returns a map of namespaced tool name → entry.
pub fn load_mcp_tools(
    servers: &[McpServerConfig],
    cache_dir: &Path,
) -> Result<IndexMap<String, McpToolEntry>> {
    build_tool_registry(servers, cache_dir, false)
}

/// Force re-discovery of all configured MCP servers, updating the cache.
pub async fn sync_mcp_tools(
    servers: &[McpServerConfig],
    cache_dir: &Path,
) -> Result<IndexMap<String, McpToolEntry>> {
    let mut registry = IndexMap::new();

    for server in servers {
        if server.disabled {
            continue;
        }

        let hash = config_hash(server);

        match discover_tools(server).await {
            Ok(tools) => {
                if let Err(e) = write_cache(cache_dir, &server.name, &hash, &tools) {
                    warn!("Failed to write MCP cache for '{}': {}", server.name, e);
                }
                for tool in &tools {
                    let entry = cached_tool_to_entry(&server.name, tool);
                    registry.insert(entry.declaration.name.clone(), entry);
                }
            }
            Err(e) => {
                warn!("MCP server '{}' discovery failed: {}", server.name, e);
            }
        }
    }

    Ok(registry)
}

// ---------------------------------------------------------------------------
// Tool invocation — connection pool and tools/call
// ---------------------------------------------------------------------------

use parking_lot::Mutex;
use std::sync::OnceLock;

/// Global pool of live MCP server connections, keyed by server name.
static MCP_CONNECTIONS: OnceLock<Mutex<HashMap<String, McpConnection>>> = OnceLock::new();

fn connections() -> &'static Mutex<HashMap<String, McpConnection>> {
    MCP_CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get or create a live connection to the specified MCP server.
async fn get_or_connect(config: &McpServerConfig) -> Result<()> {
    // Check if we already have a live connection
    {
        let mut pool = connections().lock();
        if let Some(conn) = pool.get_mut(&config.name) {
            if conn.is_alive() {
                return Ok(());
            }
            // Dead connection — remove it
            pool.remove(&config.name);
        }
    }

    // Spawn and initialize a new connection
    let mut conn = McpConnection::spawn(config)?;

    let init_timeout = Duration::from_secs(10);
    match tokio::time::timeout(init_timeout, conn.initialize()).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            conn.shutdown().await;
            return Err(e.context(format!(
                "MCP server '{}' initialization failed",
                config.name
            )));
        }
        Err(_) => {
            conn.shutdown().await;
            bail!(
                "MCP server '{}' initialization timed out ({}s)",
                config.name,
                init_timeout.as_secs()
            );
        }
    }

    connections().lock().insert(config.name.clone(), conn);
    Ok(())
}

/// Async implementation of MCP tool call. Used directly by the agent loop's parallel
/// dispatch (avoids the `block_in_place` overhead of the sync wrapper).
pub async fn call_mcp_tool_async(
    config: &McpServerConfig,
    tool_name: &str,
    arguments: Value,
    timeout: Duration,
) -> Result<Value> {
    get_or_connect(config).await?;

    let params = json!({
        "name": tool_name,
        "arguments": arguments
    });

    // Take the connection out of the pool so we can use it across await points.
    // If another parallel call already took it, spawn a fresh one.
    let conn = {
        let mut pool = connections().lock();
        pool.remove(&config.name)
    };

    let mut conn = match conn {
        Some(c) => c,
        None => {
            // Another concurrent call has the connection — spawn a new one for this call.
            let mut new_conn = McpConnection::spawn(config)?;
            let init_timeout = Duration::from_secs(10);
            match tokio::time::timeout(init_timeout, new_conn.initialize()).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    new_conn.shutdown().await;
                    return Err(e.context(format!(
                        "MCP server '{}' initialization failed (parallel spawn)",
                        config.name
                    )));
                }
                Err(_) => {
                    new_conn.shutdown().await;
                    bail!(
                        "MCP server '{}' initialization timed out (parallel spawn)",
                        config.name
                    );
                }
            }
            new_conn
        }
    };

    let call_result = tokio::time::timeout(
        timeout,
        conn.send_request("tools/call", Some(params)),
    )
    .await;

    // Put connection back regardless of result
    let output = match call_result {
        Ok(Ok(result)) => {
            connections().lock().insert(config.name.clone(), conn);
            result
        }
        Ok(Err(e)) => {
            // Request failed — connection might be dead, don't put it back
            let _ = conn.shutdown().await;
            return Err(e);
        }
        Err(_) => {
            // Timeout — kill the connection
            let _ = conn.shutdown().await;
            bail!("MCP tool call '{}' timed out ({}s)", tool_name, timeout.as_secs());
        }
    };

    // Parse MCP CallToolResult content blocks
    parse_call_tool_result(output)
}

/// Parse an MCP CallToolResult into a serde_json::Value for the LLM.
fn parse_call_tool_result(result: Value) -> Result<Value> {
    // Check for isError
    let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);

    let content = result
        .get("content")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut text_parts: Vec<String> = Vec::new();

    for block in &content {
        match block.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            Some("image") => {
                // For now, include as data URI if present
                if let (Some(data), Some(mime)) = (
                    block.get("data").and_then(|v| v.as_str()),
                    block.get("mimeType").and_then(|v| v.as_str()),
                ) {
                    text_parts.push(format!("data:{mime};base64,{data}"));
                }
            }
            _ => {
                // Unknown content type — skip
            }
        }
    }

    let output_text = text_parts.join("\n");

    if is_error {
        Ok(json!({
            "error": {
                "type": "tool_execution_error",
                "message": output_text
            }
        }))
    } else if output_text.is_empty() {
        Ok(Value::Null)
    } else {
        // Try to parse as JSON, fall back to wrapping in {"output": ...}
        match serde_json::from_str::<Value>(&output_text) {
            Ok(v) => Ok(v),
            Err(_) => Ok(json!({"output": output_text})),
        }
    }
}

/// Call an MCP tool by its original (un-namespaced) name.
/// Spawns the server on first call, keeps it alive for subsequent calls.
/// Returns Ok(Value) always — errors are wrapped as tool_execution_error values.
pub fn call_mcp_tool(
    server_config: &McpServerConfig,
    tool_name: &str,
    arguments: Value,
    timeout: Duration,
) -> Result<Value> {
    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(call_mcp_tool_async(
            server_config,
            tool_name,
            arguments,
            timeout,
        ))
    });

    match result {
        Ok(value) => Ok(value),
        Err(e) => {
            // Convert errors to tool result format so the LLM can react
            Ok(json!({
                "error": {
                    "type": "tool_execution_error",
                    "message": format!("MCP: {e}")
                }
            }))
        }
    }
}

/// Gracefully shut down all running MCP server connections.
pub fn shutdown_all_mcp_servers() {
    let pool = match MCP_CONNECTIONS.get() {
        Some(p) => p,
        None => return,
    };

    let connections: HashMap<String, McpConnection> = {
        let mut pool = pool.lock();
        std::mem::take(&mut *pool)
    };

    if connections.is_empty() {
        return;
    }

    // Best-effort shutdown — use block_in_place if we're in a tokio context
    let shutdown = async {
        for (_name, conn) in connections {
            conn.shutdown().await;
        }
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(shutdown));
    } else {
        // Not in a tokio context — just drop (will kill children via Drop)
        // The connections are already moved out and will drop here
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_server_script() -> String {
        r#"#!/bin/bash
while IFS= read -r line; do
    id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
    method=$(echo "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
    if [ "$method" = "initialize" ]; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"test\",\"version\":\"0.1.0\"}}}"
    elif [ "$method" = "tools/list" ]; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"echo\",\"description\":\"Echo input\",\"inputSchema\":{\"type\":\"object\",\"properties\":{\"text\":{\"type\":\"string\"}},\"required\":[\"text\"]}}]}}"
    elif [ "$method" = "tools/call" ]; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}"
    fi
done
"#
        .to_string()
    }

    fn write_test_script(name: &str) -> (PathBuf, McpServerConfig) {
        let tmp_dir = std::env::temp_dir();
        let script_path = tmp_dir.join(format!("aichat_mcp_test_{name}.sh"));
        std::fs::write(&script_path, test_server_script()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let config = McpServerConfig {
            name: name.to_string(),
            command: script_path.display().to_string(),
            args: vec![],
            env: HashMap::new(),
            disabled: false,
            timeout: 10,
        };
        (script_path, config)
    }

    #[tokio::test]
    async fn test_spawn_and_initialize_echo_server() {
        let (script_path, config) = write_test_script("transport");

        let mut conn = McpConnection::spawn(&config).unwrap();

        // Initialize
        let init_result = conn.initialize().await.unwrap();
        assert!(init_result.get("protocolVersion").is_some());
        assert!(init_result.get("serverInfo").is_some());

        // tools/list
        let tools_result = conn.send_request("tools/list", None).await.unwrap();
        let tools = tools_result.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");

        // tools/call
        let call_params = json!({
            "name": "echo",
            "arguments": {"text": "hello"}
        });
        let call_result = conn
            .send_request("tools/call", Some(call_params))
            .await
            .unwrap();
        let content = call_result.get("content").unwrap().as_array().unwrap();
        assert_eq!(content[0]["text"], "hello");

        // Shutdown
        conn.shutdown().await;
        let _ = std::fs::remove_file(&script_path);
    }

    #[tokio::test]
    async fn test_discover_tools_and_cache() {
        let (script_path, config) = write_test_script("discovery");
        let cache_dir = std::env::temp_dir().join("aichat_mcp_cache_test");
        let _ = std::fs::remove_dir_all(&cache_dir);

        // First call — cache miss, should discover
        let tools = discover_tools(&config).await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description, "Echo input");

        // Write cache
        let hash = config_hash(&config);
        write_cache(&cache_dir, &config.name, &hash, &tools).unwrap();

        // Read cache back
        let cached = read_cache(&cache_dir, &config.name, &hash);
        assert!(cached.is_some());
        let cached = cached.unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].name, "echo");

        // Verify cache invalidation on config change
        let stale = read_cache(&cache_dir, &config.name, "different_hash");
        assert!(stale.is_none());

        // Cleanup
        let _ = std::fs::remove_dir_all(&cache_dir);
        let _ = std::fs::remove_file(&script_path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_load_mcp_tools_builds_registry() {
        let (script_path, config) = write_test_script("registry");
        let cache_dir = std::env::temp_dir().join("aichat_mcp_registry_test");
        let _ = std::fs::remove_dir_all(&cache_dir);

        let registry = load_mcp_tools(&[config.clone()], &cache_dir).unwrap();

        // Should have one tool with namespaced name
        assert_eq!(registry.len(), 1);
        let key = "registry__echo";
        assert!(registry.contains_key(key));
        let entry = &registry[key];
        assert_eq!(entry.server_name, "registry");
        assert_eq!(entry.original_name, "echo");
        assert_eq!(entry.declaration.name, "registry__echo");
        assert_eq!(entry.declaration.description, "Echo input");

        // Second call should hit cache (no spawn needed — but we can't easily verify
        // no spawn, so just verify it returns the same result)
        let registry2 = load_mcp_tools(&[config], &cache_dir).unwrap();
        assert_eq!(registry2.len(), 1);
        assert!(registry2.contains_key(key));

        // Cleanup
        let _ = std::fs::remove_dir_all(&cache_dir);
        let _ = std::fs::remove_file(&script_path);
    }

    #[tokio::test]
    async fn test_disabled_server_is_skipped() {
        let (script_path, mut config) = write_test_script("disabled");
        config.disabled = true;
        let cache_dir = std::env::temp_dir().join("aichat_mcp_disabled_test");
        let _ = std::fs::remove_dir_all(&cache_dir);

        let registry = load_mcp_tools(&[config], &cache_dir).unwrap();
        assert!(registry.is_empty());

        let _ = std::fs::remove_dir_all(&cache_dir);
        let _ = std::fs::remove_file(&script_path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_call_mcp_tool() {
        let (script_path, config) = write_test_script("invoke");

        let arguments = json!({"text": "world"});
        let result = call_mcp_tool(&config, "echo", arguments, Duration::from_secs(10));
        assert!(result.is_ok());
        let value = result.unwrap();
        // The mock server returns {"content": [{"type":"text","text":"hello"}]}
        // which parse_call_tool_result turns into {"output": "hello"}
        assert_eq!(value, json!({"output": "hello"}));

        // Second call should reuse connection
        let arguments2 = json!({"text": "again"});
        let result2 = call_mcp_tool(&config, "echo", arguments2, Duration::from_secs(10));
        assert!(result2.is_ok());

        // Cleanup: shutdown all and remove script
        shutdown_all_mcp_servers();
        let _ = std::fs::remove_file(&script_path);
    }

    #[test]
    fn test_parse_call_tool_result_text() {
        let result = json!({
            "content": [
                {"type": "text", "text": "line1"},
                {"type": "text", "text": "line2"}
            ]
        });
        let parsed = parse_call_tool_result(result).unwrap();
        assert_eq!(parsed, json!({"output": "line1\nline2"}));
    }

    #[test]
    fn test_parse_call_tool_result_error() {
        let result = json!({
            "isError": true,
            "content": [
                {"type": "text", "text": "something went wrong"}
            ]
        });
        let parsed = parse_call_tool_result(result).unwrap();
        assert_eq!(
            parsed,
            json!({"error": {"type": "tool_execution_error", "message": "something went wrong"}})
        );
    }

    #[test]
    fn test_parse_call_tool_result_json_output() {
        let result = json!({
            "content": [
                {"type": "text", "text": "{\"key\": \"value\"}"}
            ]
        });
        let parsed = parse_call_tool_result(result).unwrap();
        assert_eq!(parsed, json!({"key": "value"}));
    }

    #[test]
    fn test_parse_call_tool_result_empty() {
        let result = json!({"content": []});
        let parsed = parse_call_tool_result(result).unwrap();
        assert!(parsed.is_null());
    }

    #[test]
    fn test_expand_env_vars() {
        std::env::set_var("AICHAT_TEST_VAR", "hello");
        std::env::set_var("AICHAT_TEST_PATH", "/usr/bin");

        assert_eq!(expand_env_vars("$AICHAT_TEST_VAR"), "hello");
        assert_eq!(expand_env_vars("${AICHAT_TEST_VAR}"), "hello");
        assert_eq!(
            expand_env_vars("$AICHAT_TEST_PATH/server"),
            "/usr/bin/server"
        );
        assert_eq!(
            expand_env_vars("prefix_${AICHAT_TEST_VAR}_suffix"),
            "prefix_hello_suffix"
        );
        // Unknown var expands to empty
        assert_eq!(expand_env_vars("$AICHAT_NONEXISTENT_XYZ"), "");
        // No expansion needed
        assert_eq!(expand_env_vars("plain text"), "plain text");
        // Bare $ at end
        assert_eq!(expand_env_vars("cost is $"), "cost is $");

        std::env::remove_var("AICHAT_TEST_VAR");
        std::env::remove_var("AICHAT_TEST_PATH");
    }
}
