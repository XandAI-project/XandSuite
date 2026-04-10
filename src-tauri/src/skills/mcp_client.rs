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
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::process_ext::HideWindowTokio;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::Duration;

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

/// Default per-tool-call timeout: 30 minutes.
///
/// Video generation (and other long-running ComfyUI/diffusion workflows) can
/// legitimately take 10-12+ minutes.  This ceiling prevents the OS or a silent
/// network proxy from silently killing idle stdio/HTTP connections before the
/// tool has had a chance to finish.
const DEFAULT_TOOL_CALL_TIMEOUT_SECS: u64 = 1800;

pub struct McpClient {
    transport: Transport,
    id_counter: Arc<AtomicU64>,
    pub server_id: String,
    /// Maximum wall-clock time allowed for a single `tools/call` round-trip.
    /// Defaults to [`DEFAULT_TOOL_CALL_TIMEOUT_SECS`] (30 min).
    tool_call_timeout_secs: u64,
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
        cmd.hide_window();
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        for (k, v) in env_vars {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP subprocess '{}' — is '{}' on your PATH?", args.first().map(|s| s.as_str()).unwrap_or("?"), command))?;

        let stdin = child
            .stdin
            .take()
            .context("Failed to capture subprocess stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("Failed to capture subprocess stdout")?;
        let mut stderr_pipe = child.stderr.take();

        let mut client = Self {
            transport: Transport::Stdio {
                _child: child,
                stdin: Mutex::new(stdin),
                stdout: Mutex::new(BufReader::new(stdout)),
            },
            id_counter: Arc::new(AtomicU64::new(1)),
            server_id,
            tool_call_timeout_secs: DEFAULT_TOOL_CALL_TIMEOUT_SECS,
        };

        // Apply a generous timeout so a hung script doesn't block forever.
        match tokio::time::timeout(Duration::from_secs(15), client.initialize()).await {
            Ok(Ok(())) => Ok(client),
            Ok(Err(init_err)) => {
                // Drain stderr for at most 500 ms so we get the Python traceback.
                let stderr_text = if let Some(ref mut se) = stderr_pipe {
                    tokio::time::timeout(Duration::from_millis(500), async {
                        let mut buf = String::new();
                        let _ = se.read_to_string(&mut buf).await;
                        buf
                    })
                    .await
                    .unwrap_or_default()
                } else {
                    String::new()
                };
                let stderr_text = stderr_text.trim().to_string();
                if stderr_text.is_empty() {
                    Err(init_err)
                } else {
                    Err(init_err.context(format!(
                        "Python process wrote to stderr:\n{}",
                        stderr_text
                    )))
                }
            }
            Err(_) => bail!(
                "MCP subprocess timed out during initialization (15 s). \
                 Check that the script starts and responds on stdin/stdout."
            ),
        }
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
            tool_call_timeout_secs: DEFAULT_TOOL_CALL_TIMEOUT_SECS,
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
    ///
    /// The call is bounded by `tool_call_timeout_secs` (default 30 min) so that
    /// long-running tools such as video generation are given enough time to
    /// complete while still guarding against truly hung processes.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<McpToolResult> {
        let params = json!({
            "name": name,
            "arguments": arguments
        });
        let timeout = Duration::from_secs(self.tool_call_timeout_secs);
        let result = tokio::time::timeout(timeout, self.send_request("tools/call", params))
            .await
            .map_err(|_| anyhow::anyhow!(
                "MCP tool '{}' timed out after {} s ({} min). \
                 If this tool is expected to run longer, increase the tool call timeout.",
                name,
                self.tool_call_timeout_secs,
                self.tool_call_timeout_secs / 60,
            ))??;
        let tool_result: McpToolResult = serde_json::from_value(result)?;
        Ok(tool_result)
    }
}
