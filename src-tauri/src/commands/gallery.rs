use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryImage {
    pub id: String,
    pub conversation_id: String,
    pub source: String,
    pub filename: String,
    pub image_data: String,
    pub mime_type: String,
    pub prompt: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: String,
}

fn row_to_image(row: &rusqlite::Row<'_>) -> rusqlite::Result<GalleryImage> {
    Ok(GalleryImage {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        source: row.get(2)?,
        filename: row.get(3)?,
        image_data: row.get(4)?,
        mime_type: row.get(5)?,
        prompt: row.get(6)?,
        width: row.get(7)?,
        height: row.get(8)?,
        created_at: row.get(9)?,
    })
}

const SELECT_COLS: &str =
    "id, conversation_id, source, filename, image_data, mime_type, prompt, width, height, created_at";

#[tauri::command]
pub fn list_gallery_images(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<GalleryImage>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM gallery_images \
             WHERE conversation_id = ?1 ORDER BY created_at ASC"
        ))
        .map_err(|e| e.to_string())?;

    let images = stmt
        .query_map(params![conversation_id], row_to_image)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(images)
}

#[tauri::command]
pub fn list_all_gallery_images(state: State<'_, AppState>) -> Result<Vec<GalleryImage>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM gallery_images ORDER BY created_at DESC"
        ))
        .map_err(|e| e.to_string())?;

    let images = stmt
        .query_map([], row_to_image)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(images)
}

#[tauri::command]
pub fn delete_gallery_image(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute("DELETE FROM gallery_images WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct SaveUploadPayload {
    pub conversation_id: String,
    pub filename: String,
    pub image_data: String,
    pub mime_type: String,
}

#[tauri::command]
pub fn save_upload_to_gallery(
    payload: SaveUploadPayload,
    state: State<'_, AppState>,
) -> Result<GalleryImage, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "INSERT INTO gallery_images \
             (id, conversation_id, source, filename, image_data, mime_type, prompt, width, height, created_at) \
             VALUES (?1, ?2, 'upload', ?3, ?4, ?5, NULL, NULL, NULL, ?6)",
            params![
                id,
                payload.conversation_id,
                payload.filename,
                payload.image_data,
                payload.mime_type,
                now
            ],
        )
        .map_err(|e| e.to_string())?;

    Ok(GalleryImage {
        id,
        conversation_id: payload.conversation_id,
        source: "upload".to_string(),
        filename: payload.filename,
        image_data: payload.image_data,
        mime_type: payload.mime_type,
        prompt: None,
        width: None,
        height: None,
        created_at: now,
    })
}
