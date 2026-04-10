use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use rusqlite::params;
use std::sync::Arc;
use uuid::Uuid;

use crate::commands::personas::{CreatePersonaInput, UpdatePersonaInput};
use crate::models::Persona;
use crate::state::AppState;

fn row_to_persona(row: &rusqlite::Row<'_>) -> rusqlite::Result<Persona> {
    let rag_json: String = row.get(6)?;
    let rag_collection_ids: Vec<String> =
        serde_json::from_str(&rag_json).unwrap_or_default();
    let memory_enabled_int: i64 = row.get(7)?;
    Ok(Persona {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        avatar: row.get(3)?,
        system_prompt: row.get(4)?,
        model_id: row.get(5)?,
        rag_collection_ids,
        memory_enabled: memory_enabled_int != 0,
        memory_collection_id: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

pub async fn list_personas(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Persona>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn
        .prepare(
            "SELECT id, name, description, avatar, system_prompt, model_id,
                    rag_collection_ids, memory_enabled, memory_collection_id,
                    created_at, updated_at
             FROM personas ORDER BY created_at ASC",
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let personas: Vec<Persona> = stmt
        .query_map([], row_to_persona)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(personas))
}

pub async fn get_persona(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Option<Persona>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let result = db.conn.query_row(
        "SELECT id, name, description, avatar, system_prompt, model_id,
                rag_collection_ids, memory_enabled, memory_collection_id,
                created_at, updated_at
         FROM personas WHERE id = ?1",
        params![id],
        row_to_persona,
    );

    match result {
        Ok(p) => Ok(Json(Some(p))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Json(None)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn create_persona(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreatePersonaInput>,
) -> Result<Json<Persona>, (StatusCode, String)> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let rag_json = serde_json::to_string(&input.rag_collection_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let db = state.db.lock().unwrap();
    db.conn.execute(
        "INSERT INTO personas
         (id, name, description, avatar, system_prompt, model_id,
          rag_collection_ids, memory_enabled, memory_collection_id,
          created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?9)",
        params![
            id, input.name, input.description, input.avatar,
            input.system_prompt, input.model_id, rag_json,
            input.memory_enabled as i64, now,
        ],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let memory_collection_id = if input.memory_enabled {
        let cid = format!("persona_memory_{}", id);
        let cname = format!("{} Memory", input.name);
        db.conn.execute(
            "INSERT OR IGNORE INTO rag_collections (id, name, description, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![cid, cname, format!("Memory for persona {}", input.name), now],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        db.conn.execute(
            "UPDATE personas SET memory_collection_id = ?1 WHERE id = ?2",
            params![cid, id],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Some(cid)
    } else {
        None
    };

    Ok(Json(Persona {
        id,
        name: input.name,
        description: input.description,
        avatar: input.avatar,
        system_prompt: input.system_prompt,
        model_id: input.model_id,
        rag_collection_ids: input.rag_collection_ids,
        memory_enabled: input.memory_enabled,
        memory_collection_id,
        created_at: now.clone(),
        updated_at: now,
    }))
}

pub async fn update_persona(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(mut input): Json<UpdatePersonaInput>,
) -> Result<Json<Persona>, (StatusCode, String)> {
    input.id = id;
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    if let Some(name) = &input.name {
        db.conn.execute("UPDATE personas SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, now, input.id]).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(desc) = &input.description {
        db.conn.execute("UPDATE personas SET description = ?1, updated_at = ?2 WHERE id = ?3",
            params![desc, now, input.id]).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(avatar) = &input.avatar {
        db.conn.execute("UPDATE personas SET avatar = ?1, updated_at = ?2 WHERE id = ?3",
            params![avatar, now, input.id]).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(sp) = &input.system_prompt {
        db.conn.execute("UPDATE personas SET system_prompt = ?1, updated_at = ?2 WHERE id = ?3",
            params![sp, now, input.id]).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(mid) = &input.model_id {
        db.conn.execute("UPDATE personas SET model_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![mid, now, input.id]).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(ids) = &input.rag_collection_ids {
        let rag_json = serde_json::to_string(ids)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        db.conn.execute("UPDATE personas SET rag_collection_ids = ?1, updated_at = ?2 WHERE id = ?3",
            params![rag_json, now, input.id]).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    if let Some(mem) = input.memory_enabled {
        db.conn.execute("UPDATE personas SET memory_enabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![mem as i64, now, input.id]).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let persona = db.conn.query_row(
        "SELECT id, name, description, avatar, system_prompt, model_id,
                rag_collection_ids, memory_enabled, memory_collection_id,
                created_at, updated_at
         FROM personas WHERE id = ?1",
        params![input.id],
        row_to_persona,
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(persona))
}

pub async fn delete_persona(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn.execute("DELETE FROM personas WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}
