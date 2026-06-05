use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::state::AppState;

pub async fn list_hf_models(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare("SELECT id, name, author, description, tags, downloads, likes, last_modified, gguf_files, is_downloaded, local_path FROM hf_models ORDER BY downloads DESC")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            let tags: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
            let gguf: String = row.get::<_, Option<String>>(8)?.unwrap_or_default();
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "author": row.get::<_, String>(2)?,
                "description": row.get::<_, Option<String>>(3)?,
                "tags": serde_json::from_str::<Value>(&tags).unwrap_or_default(),
                "downloads": row.get::<_, Option<i64>>(5)?,
                "likes": row.get::<_, Option<i64>>(6)?,
                "last_modified": row.get::<_, Option<String>>(7)?,
                "gguf_files": serde_json::from_str::<Value>(&gguf).unwrap_or_default(),
                "is_downloaded": row.get::<_, bool>(9)?,
                "local_path": row.get::<_, Option<String>>(10)?,
            }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(serde_json::json!(rows)))
}

pub async fn list_downloaded_models(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let data_dir = state.data_dir.join("models");
    let mut models: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("gguf") {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                models.push(serde_json::json!({
                    "path": path.to_string_lossy(),
                    "filename": path.file_name().unwrap_or_default().to_string_lossy(),
                    "size_bytes": size,
                }));
            }
        }
    }
    Ok(Json(serde_json::json!(models)))
}

#[derive(Deserialize)]
pub struct LoadModelBody {
    pub model_path: String,
}

pub async fn load_model(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoadModelBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let settings = state.settings.lock().unwrap().clone();
    let mut srv = state.server.lock().await;
    srv.clear_adopted();
    srv.start(&body.model_path, &settings, &state.data_dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    drop(srv);

    let port = settings.llama_server_port;
    let _ = state.engine.connect_remote(format!("http://127.0.0.1:{}", port), None, None).await;

    let mut s = state.settings.lock().unwrap();
    s.last_server_model = Some(body.model_path.clone());
    let json = serde_json::to_string(&*s).unwrap_or_default();
    let db = state.db.lock().unwrap();
    let _ = db.conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('app_settings', ?1)",
        rusqlite::params![json],
    );

    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct ConnectRemoteBody {
    pub url: String,
    pub api_key: Option<String>,
    pub model_id: Option<String>,
}

pub async fn connect_remote_server(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ConnectRemoteBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state
        .engine
        .connect_remote(body.url, body.api_key, body.model_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn is_engine_loaded(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let loaded = state.engine.get_remote().is_some();
    Ok(Json(serde_json::json!({ "loaded": loaded })))
}

pub async fn delete_model(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(filename): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let path = state.data_dir.join("models").join(&filename);
    std::fs::remove_file(&path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}
