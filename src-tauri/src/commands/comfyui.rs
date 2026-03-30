use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfyWorkflow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub workflow_json: String,
    pub created_at: String,
}

#[tauri::command]
pub fn list_comfyui_workflows(state: State<'_, AppState>) -> Result<Vec<ComfyWorkflow>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, name, description, workflow_json, created_at \
             FROM comfyui_workflows ORDER BY name ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ComfyWorkflow {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                workflow_json: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

#[derive(Debug, Deserialize)]
pub struct SaveWorkflowPayload {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub workflow_json: String,
}

#[tauri::command]
pub fn save_comfyui_workflow(
    payload: SaveWorkflowPayload,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Validate JSON
    serde_json::from_str::<serde_json::Value>(&payload.workflow_json)
        .map_err(|e| format!("Invalid workflow JSON: {}", e))?;

    let id = payload
        .id
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = chrono::Utc::now().to_rfc3339();

    db.conn
        .execute(
            "INSERT INTO comfyui_workflows (id, name, description, workflow_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                name          = excluded.name,
                description   = excluded.description,
                workflow_json = excluded.workflow_json",
            params![id, payload.name, payload.description, payload.workflow_json, now],
        )
        .map_err(|e| e.to_string())?;

    Ok(id)
}

#[tauri::command]
pub fn delete_comfyui_workflow(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.conn
        .execute("DELETE FROM comfyui_workflows WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
