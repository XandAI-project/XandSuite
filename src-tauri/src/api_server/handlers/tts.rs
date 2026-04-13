use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::commands::tts::{TtsStatus};
use crate::state::AppState;

pub async fn get_tts_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TtsStatus>, (StatusCode, String)> {
    let settings = state.settings.lock().unwrap().clone();
    let mut tts = state.tts.lock().await;
    let running = tts.is_running();
    let healthy = if running { tts.is_healthy().await } else { false };

    Ok(Json(TtsStatus {
        enabled: settings.tts_enabled,
        models_exist: crate::tts::KokoroManager::models_exist(&state.data_dir),
        deps_ready: crate::tts::KokoroManager::deps_ready(&state.data_dir, &settings.tts_device),
        running,
        healthy,
        port: settings.tts_port,
        voice: settings.tts_voice.clone(),
        speed: settings.tts_speed,
        language: settings.tts_language.clone(),
        device: settings.tts_device.clone(),
    }))
}

pub async fn start_tts_server(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let settings = state.settings.lock().unwrap().clone();
    state
        .tts
        .lock()
        .await
        .spawn(settings.tts_port, &state.data_dir, &settings.tts_device)
        .map(|_| Json(serde_json::json!({ "success": true, "status": "starting" })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn get_tts_log(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let log = state.tts.lock().await.read_log().unwrap_or_default();
    Ok(Json(serde_json::json!({ "log": log })))
}

pub async fn stop_tts_server(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state.tts.lock().await.stop();
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct SynthesizeBody {
    pub text: String,
    pub voice: String,
    pub speed: Option<f32>,
}

/// Returns raw WAV audio bytes with Content-Type: audio/wav.
pub async fn synthesize_speech(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SynthesizeBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let settings = state.settings.lock().unwrap().clone();
    let speed = body.speed.unwrap_or(settings.tts_speed);

    {
        let mut tts = state.tts.lock().await;
        if !tts.is_running() {
            tts.spawn(settings.tts_port, &state.data_dir, &settings.tts_device)
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to start TTS: {}", e),
                    )
                })?;
        }
    }

    // Wait for the server to be ready (non-blocking spawn may still be loading)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    loop {
        let mut tts = state.tts.lock().await;
        if tts.is_healthy().await {
            break;
        }
        if !tts.is_running() {
            let log = tts.read_log().unwrap_or_default();
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("kokoro-server exited.\nLog:\n{}", log),
            ));
        }
        drop(tts);
        if std::time::Instant::now() >= deadline {
            return Err((StatusCode::GATEWAY_TIMEOUT, "TTS server not ready".into()));
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    let audio = state
        .tts
        .lock()
        .await
        .synthesize(&body.text, &body.voice, speed)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "audio/wav")], audio))
}

pub async fn setup_tts_deps(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let device = state.settings.lock().unwrap().tts_device.clone();
    let log_path = std::env::temp_dir().join("kokoro-setup.log");

    let mut child = crate::tts::KokoroManager::spawn_setup(&state.data_dir, &device, &log_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let app_clone = state.app_handle.clone();
    let log_path_clone = log_path.clone();

    tauri::async_runtime::spawn(async move {
        use tauri::Emitter;
        let mut last_byte: u64 = 0;
        loop {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    if let Ok(contents) = std::fs::read_to_string(&log_path_clone) {
                        let bytes = contents.len() as u64;
                        if bytes > last_byte {
                            for line in contents[last_byte as usize..].lines() {
                                let _ = app_clone.emit("setup_tts_progress", line);
                            }
                        }
                    }
                    if !exit_status.success() {
                        let _ = app_clone.emit(
                            "setup_tts_progress",
                            format!("Setup failed with exit code {}", exit_status),
                        );
                    }
                    return;
                }
                Ok(None) => {
                    if let Ok(contents) = std::fs::read_to_string(&log_path_clone) {
                        let bytes = contents.len() as u64;
                        if bytes > last_byte {
                            for line in contents[last_byte as usize..].lines() {
                                let _ = app_clone.emit("setup_tts_progress", line);
                            }
                            last_byte = bytes;
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
                Err(_) => return,
            }
        }
    });

    Ok(Json(serde_json::json!({ "success": true, "status": "setup_started" })))
}

pub async fn download_tts_models(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let (tx, mut rx) =
        tokio::sync::mpsc::channel::<crate::models::DownloadProgress>(32);

    let app_clone = state.app_handle.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Emitter;
        while let Some(progress) = rx.recv().await {
            let _ = app_clone.emit("download_tts_progress", &progress);
        }
    });

    let device = state.settings.lock().unwrap().tts_device.clone();
    crate::tts::downloader::download_kokoro_models(&state.data_dir, &device, tx)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}
