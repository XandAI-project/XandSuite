/// SkillsManager — owns all connected MCP server connections and provides
/// a unified view of available tools.  Configuration is persisted in the
/// app SQLite database (via AppDb.set_setting / get_setting).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

use super::mcp_client::{McpClient, McpTool, McpToolResult};

// ── Serializable config (persisted in DB) ─────────────────────────────────

/// How the MCP server process is started / connected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
    },
    Http {
        url: String,
        auth: Option<String>,
    },
}

/// A registered MCP server entry (persisted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    pub transport: McpTransport,
    pub builtin: bool,
    pub enabled: bool,
    pub icon: String,
}

/// The full persisted config blob.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsConfig {
    pub servers: Vec<McpServerConfig>,
}

// ── Runtime state ──────────────────────────────────────────────────────────

/// A connected server: its config + live MCP client + cached tool list.
struct ConnectedServer {
    config: McpServerConfig,
    client: McpClient,
    tools: Vec<McpTool>,
}

// ── SkillsManager ─────────────────────────────────────────────────────────

pub struct SkillsManager {
    tools_dir: PathBuf,
    workspace_dir: PathBuf,
    /// server_id → connected session
    servers: RwLock<HashMap<String, ConnectedServer>>,
}

impl SkillsManager {
    pub fn new(tools_dir: PathBuf, workspace_dir: PathBuf) -> Self {
        Self {
            tools_dir,
            workspace_dir,
            servers: RwLock::new(HashMap::new()),
        }
    }

    // ── Tool-dir template expansion ────────────────────────────────────────

    fn expand_args(&self, args: &[String]) -> Vec<String> {
        let tools_str = self.tools_dir.to_string_lossy();
        args.iter()
            .map(|a| a.replace("${TOOLS_DIR}", &tools_str))
            .collect()
    }

    // ── Connect a single server ────────────────────────────────────────────

    pub async fn connect_server(&self, cfg: McpServerConfig) -> Result<()> {
        if !cfg.enabled {
            return Ok(());
        }
        let workspace_env = vec![(
            "XANDSUITE_WORKSPACE".to_string(),
            self.workspace_dir.to_string_lossy().to_string(),
        )];

        let client = match &cfg.transport {
            McpTransport::Stdio { command, args } => {
                let expanded = self.expand_args(args);
                McpClient::connect_stdio(
                    cfg.id.clone(),
                    command,
                    &expanded,
                    workspace_env,
                )
                .await
                .with_context(|| format!("Failed to connect to MCP server '{}'", cfg.name))?
            }
            McpTransport::Http { url, auth } => {
                McpClient::connect_http(cfg.id.clone(), url.clone(), auth.clone())
                    .await
                    .with_context(|| format!("Failed to connect to HTTP MCP server '{}'", cfg.name))?
            }
        };

        let tools = client
            .list_tools()
            .await
            .with_context(|| format!("Failed to list tools for '{}'", cfg.name))?;

        log::info!(
            "MCP server '{}' connected with {} tool(s)",
            cfg.name,
            tools.len()
        );

        let mut guard = self.servers.write().await;
        guard.insert(
            cfg.id.clone(),
            ConnectedServer { config: cfg, client, tools },
        );
        Ok(())
    }

    /// Disconnect and remove a server by ID.
    pub async fn disconnect_server(&self, server_id: &str) {
        let mut guard = self.servers.write().await;
        guard.remove(server_id);
    }

    // ── Tool discovery ─────────────────────────────────────────────────────

    /// All tools from all connected servers, with the server_id tagged on.
    pub async fn all_tools(&self) -> Vec<TaggedTool> {
        let guard = self.servers.read().await;
        guard
            .values()
            .flat_map(|s| {
                s.tools.iter().map(|t| TaggedTool {
                    server_id: s.config.id.clone(),
                    server_name: s.config.name.clone(),
                    tool: t.clone(),
                })
            })
            .collect()
    }

    /// Connected server configurations with connection status.
    pub async fn server_statuses(&self) -> Vec<ServerStatus> {
        let guard = self.servers.read().await;
        guard
            .values()
            .map(|s| ServerStatus {
                config: s.config.clone(),
                connected: true,
                tool_count: s.tools.len(),
            })
            .collect()
    }

    // ── Tool execution ─────────────────────────────────────────────────────

    /// Execute a tool, routing to the correct MCP server.
    /// The tool name format is `server_id::tool_name` (or just `tool_name`
    /// if it is unique across all servers).
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult> {
        let guard = self.servers.read().await;
        let server = guard
            .get(server_id)
            .with_context(|| format!("MCP server '{}' is not connected", server_id))?;
        server.client.call_tool(tool_name, arguments).await
    }

    /// Find which server owns a tool by name (returns first match).
    pub async fn find_server_for_tool(&self, tool_name: &str) -> Option<String> {
        let guard = self.servers.read().await;
        for (sid, s) in guard.iter() {
            if s.tools.iter().any(|t| t.name == tool_name) {
                return Some(sid.clone());
            }
        }
        None
    }
}

// ── Public output types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggedTool {
    pub server_id: String,
    pub server_name: String,
    pub tool: McpTool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub config: McpServerConfig,
    pub connected: bool,
    pub tool_count: usize,
}
