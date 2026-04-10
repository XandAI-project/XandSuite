use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::commands::whisper::WhisperStatus;
use crate::state::AppState;

pub async fn get_whisper_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WhisperStatus>, (StatusCode, String)> {
    let settings = state.settings.lock().unwrap().clone();
    let mut whisper = state.whisper.lock().await;
    Ok(Json(WhisperStatus {
        enabled: settings.whisper_enabled,
        binary_exists: crate::whisper::WhisperManager::binary_exists(&state.data_dir),
        model_path: settings.whisper_model_path.clone(),
        running: whisper.is_running(),
        port: settings.whisper_port,
    }))
}

pub async fn start_whisper_server(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let settings = state.settings.lock().unwrap().clone();
    let model_path = settings
        .whisper_model_path
        .as_deref()
        .ok_or((StatusCode::BAD_REQUEST, "No Whisper model configured. Download a model first.".to_string()))?
        .to_string();
    state
        .whisper
        .lock()
        .await
        .start(&model_path, settings.whisper_port, &state.data_dir)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn stop_whisper_server(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state.whisper.lock().await.stop();
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct TranscribeBody {
    /// Base64-encoded audio bytes
    pub audio_data: String,
    pub ext: String,
}

pub async fn transcribe_audio(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TranscribeBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let audio_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &body.audio_data,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid base64 audio: {}", e)))?;

    let settings = state.settings.lock().unwrap().clone();
    {
        let mut whisper = state.whisper.lock().await;
        if !whisper.is_running() {
            let model_path = settings
                .whisper_model_path
                .as_deref()
                .ok_or((StatusCode::BAD_REQUEST, "No Whisper model configured.".to_string()))?
                .to_string();
            whisper
                .start(&model_path, settings.whisper_port, &state.data_dir)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start whisper-server: {}", e)))?;
        }
    }

    let language = settings.whisper_language.clone();
    let text = state
        .whisper
        .lock()
        .await
        .transcribe(&audio_bytes, &body.ext, &language)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "text": text })))
}

#[derive(Deserialize)]
pub struct DownloadModelBody {
    pub size: String,
}

pub async fn download_whisper_binary(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let variant = state.settings.lock().unwrap().whisper_variant.clone();
    state.whisper.lock().await.stop();
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let bin_dir = state.data_dir.join("bin").join("whisper");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::models::DownloadProgress>(32);

    let app_clone = state.app_handle.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Emitter;
        while let Some(progress) = rx.recv().await {
            let _ = app_clone.emit("server_binary_progress", &progress);
        }
    });

    crate::whisper::downloader::download_whisper_binary(&bin_dir, &variant, tx)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn download_whisper_model(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DownloadModelBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let models_dir = state.data_dir.join("whisper-models");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::models::DownloadProgress>(32);

    let app_clone = state.app_handle.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Emitter;
        while let Some(progress) = rx.recv().await {
            let _ = app_clone.emit("download_progress", &progress);
        }
    });

    crate::whisper::downloader::download_whisper_model(&body.size, &models_dir, tx)
        .await
        .map(|dest| Json(serde_json::json!({ "path": dest.to_string_lossy() })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}
