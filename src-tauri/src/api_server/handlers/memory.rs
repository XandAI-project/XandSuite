use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rusqlite::params;
use serde_json::Value;
use std::sync::Arc;

use crate::models::MEMORY_COLLECTION_ID;
use crate::state::AppState;

pub async fn list_memory_entries(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // Retrieve memory chunks from the internal SQLite rag_chunks table
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, collection_id, content, document_id, created_at
             FROM rag_chunks WHERE collection_id = ?1 ORDER BY rowid DESC LIMIT 500",
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let x: Vec<Value> = stmt
        .query_map(params![MEMORY_COLLECTION_ID], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "collection_id": row.get::<_, String>(1)?,
                "content": row.get::<_, String>(2)?,
                "document_id": row.get::<_, String>(3)?,
            }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(serde_json::json!(x)))
}

pub async fn delete_memory_entry(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "DELETE FROM rag_chunks WHERE id = ?1 AND collection_id = ?2",
            params![id, MEMORY_COLLECTION_ID],
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn clear_memory_entries(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "DELETE FROM rag_chunks WHERE collection_id = ?1",
            params![MEMORY_COLLECTION_ID],
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}
