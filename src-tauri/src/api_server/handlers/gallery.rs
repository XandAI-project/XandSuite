use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    Json,
};
use base64::Engine as _;
use rusqlite::params;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct GalleryQuery {
    pub conversation_id: Option<String>,
}

fn query_gallery(db: &crate::db::AppDb, conv_id: Option<&str>) -> rusqlite::Result<Vec<Value>> {
    if let Some(cid) = conv_id {
        let mut stmt = db.conn.prepare(
            "SELECT id, conversation_id, source, filename, mime_type, data, created_at
             FROM gallery_images WHERE conversation_id = ?1 ORDER BY created_at DESC",
        )?;
        let x: Vec<Value> = stmt.query_map(params![cid], gallery_row)?.filter_map(|r| r.ok()).collect();
        Ok(x)
    } else {
        let mut stmt = db.conn.prepare(
            "SELECT id, conversation_id, source, filename, mime_type, data, created_at
             FROM gallery_images ORDER BY created_at DESC",
        )?;
        let x: Vec<Value> = stmt.query_map([], gallery_row)?.filter_map(|r| r.ok()).collect();
        Ok(x)
    }
}

pub async fn list_gallery_images(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GalleryQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let rows = query_gallery(&db, q.conversation_id.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!(rows)))
}

pub async fn list_all_gallery_images(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let rows = query_gallery(&db, None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!(rows)))
}

pub async fn delete_gallery_image(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute("DELETE FROM gallery_images WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn upload_gallery_image(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mut filename = String::from("upload.jpg");
    let mut mime_type = String::from("image/jpeg");
    let mut data: Option<String> = None;
    let mut conversation_id: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "conversation_id" => {
                conversation_id = field
                    .text()
                    .await
                    .ok()
                    .filter(|s| !s.is_empty());
            }
            "file" => {
                filename = field
                    .file_name()
                    .unwrap_or("upload.jpg")
                    .to_string();
                mime_type = field
                    .content_type()
                    .unwrap_or("image/jpeg")
                    .to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                data = Some(base64::engine::general_purpose::STANDARD.encode(&bytes));
            }
            _ => {}
        }
    }

    let data = data.ok_or_else(|| (StatusCode::BAD_REQUEST, "No file uploaded".to_string()))?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    {
        let db = state.db.lock().unwrap();
        db.conn
            .execute(
                "INSERT INTO gallery_images (id, conversation_id, source, filename, mime_type, data, created_at)
                 VALUES (?1, ?2, 'upload', ?3, ?4, ?5, ?6)",
                params![id, conversation_id, filename, mime_type, data, now],
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(Json(serde_json::json!({ "id": id, "filename": filename, "created_at": now })))
}

fn gallery_row(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "conversation_id": row.get::<_, Option<String>>(1)?,
        "source": row.get::<_, String>(2)?,
        "filename": row.get::<_, String>(3)?,
        "mime_type": row.get::<_, String>(4)?,
        "data": row.get::<_, String>(5)?,
        "created_at": row.get::<_, String>(6)?,
    }))
}
