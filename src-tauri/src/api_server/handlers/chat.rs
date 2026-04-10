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

// ─── Conversation endpoints ───────────────────────────────────────────────────

pub async fn list_conversations(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, title, model_id, system_prompt, created_at, updated_at
             FROM conversations ORDER BY updated_at DESC",
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "model_id": row.get::<_, Option<String>>(2)?,
                "system_prompt": row.get::<_, Option<String>>(3)?,
                "created_at": row.get::<_, String>(4)?,
                "updated_at": row.get::<_, String>(5)?,
            }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(serde_json::json!(rows)))
}

#[derive(Deserialize)]
pub struct CreateConversationBody {
    title: Option<String>,
    system_prompt: Option<String>,
}

pub async fn create_conversation(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateConversationBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let title = body.title.unwrap_or_else(|| "New Conversation".to_string());

    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "INSERT INTO conversations (id, title, system_prompt, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, title, body.system_prompt, now],
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "id": id, "title": title, "created_at": now })))
}

pub async fn get_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    let conv: Value = db
        .conn
        .query_row(
            "SELECT id, title, model_id, system_prompt, created_at, updated_at
             FROM conversations WHERE id = ?1",
            params![id],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "model_id": row.get::<_, Option<String>>(2)?,
                    "system_prompt": row.get::<_, Option<String>>(3)?,
                    "created_at": row.get::<_, String>(4)?,
                    "updated_at": row.get::<_, String>(5)?,
                }))
            },
        )
        .map_err(|_| (StatusCode::NOT_FOUND, "Conversation not found".to_string()))?;

    let mut msg_stmt = db
        .conn
        .prepare(
            "SELECT id, role, content, created_at, metadata, tool_steps
             FROM messages WHERE conversation_id = ?1 ORDER BY rowid ASC",
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let messages: Vec<Value> = msg_stmt
        .query_map(params![id], |row| {
            let meta_str: Option<String> = row.get(4)?;
            let tool_str: Option<String> = row.get(5)?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "role": row.get::<_, String>(1)?,
                "content": row.get::<_, String>(2)?,
                "created_at": row.get::<_, String>(3)?,
                "metadata": meta_str.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                "tool_steps": tool_str.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
            }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let mut result = conv.as_object().unwrap().clone();
    result.insert("messages".to_string(), serde_json::json!(messages));
    Ok(Json(Value::Object(result)))
}

#[derive(Deserialize)]
pub struct UpdateConversationBody {
    title: Option<String>,
    system_prompt: Option<String>,
}

pub async fn update_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateConversationBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let now = chrono::Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    if let Some(title) = &body.title {
        db.conn
            .execute(
                "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![title, now, id],
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(prompt) = &body.system_prompt {
        let existing: Option<i64> = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND role = 'system'",
                params![id],
                |row| row.get(0),
            )
            .ok();
        if existing.unwrap_or(0) > 0 {
            db.conn
                .execute(
                    "UPDATE messages SET content = ?1 WHERE conversation_id = ?2 AND role = 'system'",
                    params![prompt, id],
                )
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        } else {
            let msg_id = Uuid::new_v4().to_string();
            let ts = chrono::Utc::now().to_rfc3339();
            db.conn
                .execute(
                    "INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, 'system', ?3, ?4)",
                    params![msg_id, id, prompt, ts],
                )
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute("DELETE FROM messages WHERE conversation_id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    db.conn
        .execute("DELETE FROM conversations WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn truncate_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "DELETE FROM messages WHERE conversation_id = ?1 AND role != 'system'",
            params![id],
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

// ─── Send message ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SendMessageBody {
    pub conversation_id: String,
    pub content: String,
    #[serde(default)]
    pub use_rag: bool,
    pub rag_collection_id: Option<String>,
    pub use_skills: Option<bool>,
    pub attachments: Option<Vec<String>>,
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SendMessageBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let app_handle = state.app_handle.clone();

    // Call the same inner function as the Tauri command
    let result = crate::commands::chat::send_message_inner(
        app_handle,
        body.conversation_id,
        body.content,
        body.use_rag,
        body.rag_collection_id,
        body.use_skills,
        body.attachments,
        &state,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({ "assistant_msg_id": result })))
}

// ─── Tool steps ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SaveToolStepsBody {
    pub tool_steps_json: String,
}

pub async fn save_message_tool_steps(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
    Json(body): Json<SaveToolStepsBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "UPDATE messages SET tool_steps = ?1 WHERE id = ?2",
            params![body.tool_steps_json, message_id],
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}
