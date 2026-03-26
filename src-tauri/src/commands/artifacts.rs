use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Artifact {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub title: String,
    pub artifact_type: String,
    pub language: Option<String>,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[tauri::command]
pub fn save_artifact(
    conversation_id: String,
    message_id: Option<String>,
    title: String,
    artifact_type: String,
    language: Option<String>,
    content: String,
    state: State<'_, AppState>,
) -> Result<Artifact, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let db = state.db.lock().unwrap();
    db.conn.execute(
        "INSERT INTO artifacts (id, conversation_id, message_id, title, artifact_type, language, content, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![id, conversation_id, message_id, title, artifact_type, language, content, now],
    ).map_err(|e| e.to_string())?;

    Ok(Artifact {
        id,
        conversation_id,
        message_id,
        title,
        artifact_type,
        language,
        content,
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub fn list_artifacts(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Artifact>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn.prepare(
        "SELECT id, conversation_id, message_id, title, artifact_type, language, content, created_at, updated_at
         FROM artifacts WHERE conversation_id = ?1 ORDER BY created_at ASC"
    ).map_err(|e| e.to_string())?;

    let artifacts = stmt.query_map(params![conversation_id], |row| {
        Ok(Artifact {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            message_id: row.get(2)?,
            title: row.get(3)?,
            artifact_type: row.get(4)?,
            language: row.get(5)?,
            content: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(artifacts)
}

#[tauri::command]
pub fn list_all_artifacts(
    state: State<'_, AppState>,
) -> Result<Vec<Artifact>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn.prepare(
        "SELECT id, conversation_id, message_id, title, artifact_type, language, content, created_at, updated_at
         FROM artifacts ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let artifacts = stmt.query_map([], |row| {
        Ok(Artifact {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            message_id: row.get(2)?,
            title: row.get(3)?,
            artifact_type: row.get(4)?,
            language: row.get(5)?,
            content: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(artifacts)
}

#[tauri::command]
pub fn delete_artifact(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn.execute("DELETE FROM artifacts WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn update_artifact(
    id: String,
    title: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<Artifact, String> {
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    db.conn.execute(
        "UPDATE artifacts SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, now, id],
    ).map_err(|e| e.to_string())?;

    let artifact = db.conn.query_row(
        "SELECT id, conversation_id, message_id, title, artifact_type, language, content, created_at, updated_at
         FROM artifacts WHERE id = ?1",
        params![id],
        |row| Ok(Artifact {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            message_id: row.get(2)?,
            title: row.get(3)?,
            artifact_type: row.get(4)?,
            language: row.get(5)?,
            content: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        }),
    ).map_err(|e| e.to_string())?;

    Ok(artifact)
}
