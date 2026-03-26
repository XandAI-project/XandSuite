/// MCP (Model Context Protocol) client implementation.
///
/// Supports two transports:
///   - Stdio: spawns a subprocess and communicates via stdin/stdout using
///     line-delimited JSON-RPC 2.0.
///   - Http: sends JSON-RPC requests to a remote HTTP MCP server.
///
/// Protocol flow: initialize → tools/list → (tools/call)*

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

// ── JSON-RPC 2.0 primitives ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    id: Option<Value>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// ── MCP domain types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub content: Vec<McpContent>,
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
}

impl McpToolResult {
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── Transport abstraction ──────────────────────────────────────────────────

enum Transport {
    Stdio {
        _child: Child,
        stdin: Mutex<ChildStdin>,
        stdout: Mutex<BufReader<ChildStdout>>,
    },
    Http {
        client: reqwest::Client,
        url: String,
        auth: Option<String>,
    },
}

// ── McpClient ─────────────────────────────────────────────────────────────

pub struct McpClient {
    transport: Transport,
    id_counter: Arc<AtomicU64>,
    pub server_id: String,
}

impl McpClient {
    /// Spawn a local Python (or any) subprocess and perform the MCP initialize handshake.
    pub async fn connect_stdio(
        server_id: String,
        command: &str,
        args: &[String],
        env_vars: Vec<(String, String)>,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null()); // suppress Python noise

        for (k, v) in env_vars {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().context("Failed to spawn MCP subprocess")?;
        let stdin = child
            .stdin
            .take()
            .context("Failed to capture subprocess stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("Failed to capture subprocess stdout")?;

        let mut client = Self {
            transport: Transport::Stdio {
                _child: child,
                stdin: Mutex::new(stdin),
                stdout: Mutex::new(BufReader::new(stdout)),
            },
            id_counter: Arc::new(AtomicU64::new(1)),
            server_id,
        };

        client.initialize().await?;
        Ok(client)
    }

    /// Connect to a remote MCP server via HTTP (streamable-http transport).
    pub async fn connect_http(server_id: String, url: String, auth: Option<String>) -> Result<Self> {
        let mut client = Self {
            transport: Transport::Http {
                client: reqwest::Client::new(),
                url,
                auth,
            },
            id_counter: Arc::new(AtomicU64::new(1)),
            server_id,
        };
        client.initialize().await?;
        Ok(client)
    }

    // ── Internal JSON-RPC helpers ──────────────────────────────────────────

    async fn send_request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.id_counter.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        match &self.transport {
            Transport::Stdio { stdin, stdout, .. } => {
                let line = serde_json::to_string(&req)? + "\n";
                {
                    let mut w = stdin.lock().await;
                    w.write_all(line.as_bytes()).await?;
                    w.flush().await?;
                }
                let mut resp_line = String::new();
                {
                    let mut r = stdout.lock().await;
                    // Skip any notification lines (no "id" field or id is null)
                    loop {
                        resp_line.clear();
                        r.read_line(&mut resp_line).await?;
                        let v: Value = serde_json::from_str(resp_line.trim()).unwrap_or(Value::Null);
                        if v.get("id").map(|x| !x.is_null()).unwrap_or(false) {
                            break;
                        }
                        if resp_line.is_empty() {
                            bail!("MCP stdio subprocess closed its stdout");
                        }
                    }
                }
                let resp: JsonRpcResponse = serde_json::from_str(resp_line.trim())?;
                if let Some(err) = resp.error {
                    bail!("MCP error {}: {}", err.code, err.message);
                }
                Ok(resp.result.unwrap_or(Value::Null))
            }
            Transport::Http { client, url, auth } => {
                let mut builder = client.post(url).json(&req);
                if let Some(token) = auth {
                    builder = builder.bearer_auth(token);
                }
                let response = builder.send().await?;
                if !response.status().is_success() {
                    bail!("HTTP MCP error: {}", response.status());
                }
                let resp: JsonRpcResponse = response.json().await?;
                if let Some(err) = resp.error {
                    bail!("MCP error {}: {}", err.code, err.message);
                }
                Ok(resp.result.unwrap_or(Value::Null))
            }
        }
    }

    async fn initialize(&mut self) -> Result<()> {
        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "XandSuite",
                "version": "1.0.0"
            }
        });
        self.send_request("initialize", params).await?;
        // Send initialized notification (fire and forget for stdio)
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        if let Transport::Stdio { stdin, .. } = &self.transport {
            let line = serde_json::to_string(&notif)? + "\n";
            let mut w = stdin.lock().await;
            let _ = w.write_all(line.as_bytes()).await;
            let _ = w.flush().await;
        }
        Ok(())
    }

    // ── Public API ─────────────────────────────────────────────────────────

    /// Discover all tools provided by this MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        let result = self.send_request("tools/list", json!({})).await?;
        let tools: Vec<McpTool> = serde_json::from_value(
            result.get("tools").cloned().unwrap_or(Value::Array(vec![])),
        )?;
        Ok(tools)
    }

    /// Execute a tool and return the result.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<McpToolResult> {
        let params = json!({
            "name": name,
            "arguments": arguments
        });
        let result = self.send_request("tools/call", params).await?;
        let tool_result: McpToolResult = serde_json::from_value(result)?;
        Ok(tool_result)
    }
}
