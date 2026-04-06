use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use rusqlite::params;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct ConversationQuery {
    pub conversation_id: Option<String>,
}

fn query_artifacts(db: &crate::db::AppDb, conv_id: Option<&str>) -> rusqlite::Result<Vec<Value>> {
    if let Some(cid) = conv_id {
        let mut stmt = db.conn.prepare(
            "SELECT id, conversation_id, message_id, title, artifact_type, language, content, created_at, updated_at
             FROM artifacts WHERE conversation_id = ?1 ORDER BY updated_at DESC",
        )?;
        let x: Vec<Value> = stmt.query_map(params![cid], artifact_row)?.filter_map(|r| r.ok()).collect();
        Ok(x)
    } else {
        let mut stmt = db.conn.prepare(
            "SELECT id, conversation_id, message_id, title, artifact_type, language, content, created_at, updated_at
             FROM artifacts ORDER BY updated_at DESC",
        )?;
        let x: Vec<Value> = stmt.query_map([], artifact_row)?.filter_map(|r| r.ok()).collect();
        Ok(x)
    }
}

pub async fn list_artifacts(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ConversationQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let rows = query_artifacts(&db, q.conversation_id.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!(rows)))
}

pub async fn list_all_artifacts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let rows = query_artifacts(&db, None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!(rows)))
}

#[derive(Deserialize)]
pub struct SaveArtifactBody {
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub title: String,
    pub artifact_type: String,
    pub language: Option<String>,
    pub content: String,
}

pub async fn save_artifact(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveArtifactBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();
    db.conn.execute(
        "INSERT INTO artifacts (id, conversation_id, message_id, title, artifact_type, language, content, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![id, body.conversation_id, body.message_id, body.title, body.artifact_type, body.language, body.content, now],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct UpdateArtifactBody {
    pub title: Option<String>,
    pub content: Option<String>,
    pub language: Option<String>,
}

pub async fn update_artifact(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateArtifactBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let now = chrono::Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();
    if let Some(title) = &body.title {
        db.conn.execute("UPDATE artifacts SET title = ?1, updated_at = ?2 WHERE id = ?3", params![title, now, id])
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(content) = &body.content {
        db.conn.execute("UPDATE artifacts SET content = ?1, updated_at = ?2 WHERE id = ?3", params![content, now, id])
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(lang) = &body.language {
        db.conn.execute("UPDATE artifacts SET language = ?1, updated_at = ?2 WHERE id = ?3", params![lang, now, id])
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn delete_artifact(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn.execute("DELETE FROM artifacts WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

fn artifact_row(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "conversation_id": row.get::<_, String>(1)?,
        "message_id": row.get::<_, Option<String>>(2)?,
        "title": row.get::<_, String>(3)?,
        "artifact_type": row.get::<_, String>(4)?,
        "language": row.get::<_, Option<String>>(5)?,
        "content": row.get::<_, String>(6)?,
        "created_at": row.get::<_, String>(7)?,
        "updated_at": row.get::<_, String>(8)?,
    }))
}
