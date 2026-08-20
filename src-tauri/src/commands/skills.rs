use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::skills::{
    McpServerConfig, McpTransport, ServerStatus, TaggedTool,
};
use crate::state::AppState;

// ── list_tools ────────────────────────────────────────────────────────────

/// Return all tools from all connected MCP servers.
#[tauri::command]
pub async fn list_tools(state: State<'_, AppState>) -> Result<Vec<TaggedTool>, String> {
    Ok(state.skills.all_tools().await)
}

// ── list_skill_servers ────────────────────────────────────────────────────

/// Return the status of all registered skill servers.
#[tauri::command]
pub async fn list_skill_servers(state: State<'_, AppState>) -> Result<Vec<ServerStatus>, String> {
    Ok(state.skills.server_statuses().await)
}

// ── add_mcp_server ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddMcpServerRequest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub transport: String, // "stdio" | "http"
    // Stdio fields
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    // HTTP fields
    pub url: Option<String>,
    pub auth: Option<String>,
    pub icon: Option<String>,
}

/// Add and immediately connect a new MCP server.
/// Persists the config to the app database.
#[tauri::command]
pub async fn add_mcp_server(
    request: AddMcpServerRequest,
    state: State<'_, AppState>,
) -> Result<ServerStatus, String> {
    let transport = match request.transport.as_str() {
        "http" => McpTransport::Http {
            url: request.url.ok_or("url is required for http transport")?,
            auth: request.auth,
        },
        _ => McpTransport::Stdio {
            command: request.command.unwrap_or_else(|| "python".to_string()),
            args: request.args.unwrap_or_default(),
        },
    };

    let cfg = McpServerConfig {
        id: request.id.clone(),
        name: request.name,
        description: request.description,
        transport,
        builtin: false,
        enabled: true,
        icon: request.icon.unwrap_or_else(|| "Plug".to_string()),
    };

    // Persist to DB
    persist_servers_config(&state, Some(cfg.clone()), None).await?;

    // Connect
    if let Err(e) = state.skills.connect_server(cfg).await {
        // Roll back the DB record we just wrote — otherwise the server list
        // would show a "connected" entry on next launch that immediately
        // fails to reconnect, with no way to remove it from the UI (removal
        // itself calls disconnect_server on a server that was never added).
        let _ = persist_servers_config(&state, None, Some(&request.id)).await;
        return Err(e.to_string());
    }

    let statuses = state.skills.server_statuses().await;
    let status = statuses
        .into_iter()
        .find(|s| s.config.id == request.id)
        .ok_or("Server connected but not found in status list")?;
    Ok(status)
}

// ── remove_mcp_server ─────────────────────────────────────────────────────

/// Disconnect and permanently remove a non-builtin MCP server.
#[tauri::command]
pub async fn remove_mcp_server(
    server_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.skills.disconnect_server(&server_id).await;
    persist_servers_config(&state, None, Some(&server_id)).await?;
    Ok(())
}

// ── reload_builtin_servers ────────────────────────────────────────────────

/// Reconnect all builtin MCP servers (e.g. after a crash or first-run setup).
#[tauri::command]
pub async fn reload_builtin_servers(state: State<'_, AppState>) -> Result<Vec<ServerStatus>, String> {
    crate::skills_init::connect_builtin_servers(&state.skills, &state.data_dir)
        .await
        .map_err(|e| e.to_string())?;
    Ok(state.skills.server_statuses().await)
}

// ── call_tool (direct invocation for testing) ─────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CallToolRequest {
    pub server_id: String,
    pub tool_name: String,
    pub arguments: Value,
}

#[derive(Debug, Serialize)]
pub struct CallToolResponse {
    pub result: String,
    pub is_error: bool,
}

/// Directly invoke a tool — useful for testing from the Skills panel.
#[tauri::command]
pub async fn call_tool_direct(
    request: CallToolRequest,
    state: State<'_, AppState>,
) -> Result<CallToolResponse, String> {
    let result = state
        .skills
        .call_tool(&request.server_id, &request.tool_name, request.arguments)
        .await
        .map_err(|e| e.to_string())?;
    Ok(CallToolResponse {
        result: result.text(),
        is_error: result.is_error,
    })
}

// ── Helper: persist server config list ────────────────────────────────────

async fn persist_servers_config(
    state: &AppState,
    add: Option<McpServerConfig>,
    remove_id: Option<&str>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    // Load existing custom servers
    let mut servers: Vec<McpServerConfig> = db
        .get_setting("mcp_servers")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if let Some(cfg) = add {
        // Replace if exists, else push
        if let Some(pos) = servers.iter().position(|s| s.id == cfg.id) {
            servers[pos] = cfg;
        } else {
            servers.push(cfg);
        }
    }
    if let Some(id) = remove_id {
        servers.retain(|s| s.id != id);
    }

    let json = serde_json::to_string(&servers).map_err(|e| e.to_string())?;
    db.set_setting("mcp_servers", &json)
        .map_err(|e| e.to_string())?;
    Ok(())
}
