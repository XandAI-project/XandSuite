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

pub async fn list_db_connections(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, name, db_type, connection_string, is_active, created_at
             FROM db_connections ORDER BY created_at DESC",
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "db_type": row.get::<_, String>(2)?,
                "connection_string": row.get::<_, String>(3)?,
                "is_active": row.get::<_, bool>(4)?,
                "created_at": row.get::<_, String>(5)?,
            }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(serde_json::json!(rows)))
}

#[derive(Deserialize)]
pub struct AddConnectionBody {
    pub name: String,
    pub db_type: String,
    pub connection_string: String,
}

pub async fn add_db_connection(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddConnectionBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "INSERT INTO db_connections (id, name, db_type, connection_string, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![id, body.name, body.db_type, body.connection_string, now],
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

pub async fn delete_db_connection(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute("DELETE FROM db_connections WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct TestConnectionBody {
    pub connection_string: String,
    pub db_type: String,
}

pub async fn test_db_connection(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<TestConnectionBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // Delegate to the existing Tauri command logic
    match crate::commands::database::test_connection_inner(&body.connection_string, &body.db_type).await {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

#[derive(Deserialize)]
pub struct QueryBody {
    pub connection_id: String,
    pub query: String,
}

pub async fn execute_db_query(
    State(state): State<Arc<AppState>>,
    Json(body): Json<QueryBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // Get connection string
    let conn_str = {
        let db = state.db.lock().unwrap();
        db.conn.query_row(
            "SELECT connection_string, db_type FROM db_connections WHERE id = ?1",
            params![body.connection_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).map_err(|_| (StatusCode::NOT_FOUND, "Connection not found".to_string()))?
    };

    let result = crate::commands::database::execute_query_inner(
        &conn_str.0,
        &conn_str.1,
        &body.query,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
