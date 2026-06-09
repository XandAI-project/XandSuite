use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::skills::McpServerConfig;
use crate::state::AppState;

pub async fn list_skill_servers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let servers = state.skills.server_statuses().await;
    Ok(Json(serde_json::to_value(servers).unwrap_or(serde_json::json!([]))))
}

pub async fn list_tools(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tools = state.skills.all_tools().await;
    Ok(Json(serde_json::to_value(tools).unwrap_or(serde_json::json!([]))))
}

pub async fn add_mcp_server(
    State(state): State<Arc<AppState>>,
    Json(config): Json<McpServerConfig>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state
        .skills
        .connect_server(config)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn remove_mcp_server(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state.skills.disconnect_server(&id).await;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct CallToolBody {
    pub tool_name: String,
    pub arguments: Value,
    pub conv_id: Option<String>,
}

pub async fn call_tool_direct(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CallToolBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let conv_id = body.conv_id.unwrap_or_else(|| "direct".to_string());
    let tool_name = body.tool_name.clone();
    let result = state
        .skills
        .call_tool(&body.tool_name, &conv_id, body.arguments)
        .await
        .map_err(|e| {
            log::error!(
                "[skills/tools/call] Tool '{}' failed (conv={}): {}",
                tool_name, conv_id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "error": e.to_string(),
                    "tool_name": tool_name,
                })
                .to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "result": result })))
}

pub async fn reload_builtin_servers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let data_dir = state.data_dir.clone();
    let skills = state.skills.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::skills_init::connect_builtin_servers(&skills, &data_dir).await {
            log::warn!("Builtin MCP servers reload error: {}", e);
        }
    });
    Ok(Json(serde_json::json!({ "started": true })))
}
