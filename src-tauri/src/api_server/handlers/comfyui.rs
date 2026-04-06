use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rusqlite::params;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::state::AppState;

pub async fn list_comfyui_workflows(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare("SELECT id, name, workflow_json FROM comfyui_workflows ORDER BY name ASC")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            let wf_str: String = row.get(2)?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "workflow": serde_json::from_str::<Value>(&wf_str).unwrap_or(serde_json::json!({})),
            }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(serde_json::json!(rows)))
}

#[derive(Deserialize)]
pub struct SaveWorkflowBody {
    pub name: String,
    pub workflow: Value,
}

pub async fn save_comfyui_workflow(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveWorkflowBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let id = Uuid::new_v4().to_string();
    let wf_json = serde_json::to_string(&body.workflow)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "INSERT INTO comfyui_workflows (id, name, workflow_json) VALUES (?1, ?2, ?3)",
            params![id, body.name, wf_json],
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

pub async fn delete_comfyui_workflow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute("DELETE FROM comfyui_workflows WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}
