use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::state::AppState;

pub async fn get_server_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mut srv = state.server.lock().await;
    let running = srv.is_running();
    let model = state.settings.lock().unwrap().last_server_model.clone();
    let port = state.settings.lock().unwrap().llama_server_port;
    Ok(Json(serde_json::json!({
        "running": running,
        "model": model,
        "port": port,
    })))
}

#[derive(Deserialize)]
pub struct StartServerBody {
    pub model_path: String,
}

pub async fn start_local_server(
    State(state): State<Arc<AppState>>,
    Json(body): Json<StartServerBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let settings = state.settings.lock().unwrap().clone();
    let mut srv = state.server.lock().await;
    srv.clear_adopted();
    srv.start(&body.model_path, &settings, &state.data_dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    drop(srv);

    let port = settings.llama_server_port;
    let _ = state.engine.connect_remote(
        format!("http://127.0.0.1:{}", port),
        None,
        None,
    ).await;

    {
        let mut s = state.settings.lock().unwrap();
        s.last_server_model = Some(body.model_path.clone());
        let json = serde_json::to_string(&*s).unwrap_or_default();
        let db = state.db.lock().unwrap();
        let _ = db.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('app_settings', ?1)",
            rusqlite::params![json],
        );
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn stop_local_server(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mut srv = state.server.lock().await;
    srv.stop();
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn detect_gpu(
    State(_state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    match crate::commands::server::detect_gpu() {
        Ok(info) => Ok(Json(serde_json::to_value(info).unwrap_or_default())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn download_llama_server(
    State(_state): State<Arc<AppState>>,
    Json(_body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // Server binary download is initiated via the Tauri desktop UI.
    // This endpoint is a stub for future REST-native implementation.
    Err((StatusCode::NOT_IMPLEMENTED, "Use the desktop app to download the server binary".to_string()))
}
