use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rusqlite::params;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::state::AppState;

pub async fn list_agent_tasks(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, title, description, status, steps, created_at, completed_at, result
             FROM agent_tasks ORDER BY created_at DESC",
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            let steps_str: Option<String> = row.get(4)?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "description": row.get::<_, String>(2)?,
                "status": row.get::<_, String>(3)?,
                "steps": steps_str.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or_default(),
                "created_at": row.get::<_, String>(5)?,
                "completed_at": row.get::<_, Option<String>>(6)?,
                "result": row.get::<_, Option<String>>(7)?,
            }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(serde_json::json!(rows)))
}

#[derive(Deserialize)]
pub struct RunAgentBody {
    pub title: String,
    pub description: String,
}

pub async fn run_agent_task(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RunAgentBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let app = state.app_handle.clone();
    let result = crate::commands::agents::run_agent_task_inner(app, body.title, body.description, &state)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "task_id": result })))
}

pub async fn delete_agent_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute("DELETE FROM agent_tasks WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn cancel_agent_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state.agent_runtime.cancel_task(&id);
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn list_task_files(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let workspace = state.data_dir.join("agent_workspace").join(&task_id);
    let mut files: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&workspace) {
        for entry in entries.flatten() {
            let path = entry.path();
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files.push(serde_json::json!({
                "name": path.file_name().unwrap_or_default().to_string_lossy(),
                "path": path.to_string_lossy(),
                "size_bytes": size,
            }));
        }
    }
    Ok(Json(serde_json::json!(files)))
}

pub async fn read_task_file(
    State(state): State<Arc<AppState>>,
    Path((task_id, file_path)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let full_path = state
        .data_dir
        .join("agent_workspace")
        .join(&task_id)
        .join(&file_path);
    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(serde_json::json!({ "content": content })))
}
