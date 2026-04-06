use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tokio::sync::mpsc;

use crate::models::DownloadProgress;
use crate::state::AppState;
use crate::whisper::downloader;

// ── Status ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct WhisperStatus {
    pub enabled: bool,
    pub binary_exists: bool,
    pub model_path: Option<String>,
    pub running: bool,
    pub port: u16,
}

#[tauri::command]
pub async fn get_whisper_status(state: State<'_, AppState>) -> Result<WhisperStatus, String> {
    let settings = state.settings.lock().unwrap().clone();
    let mut whisper = state.whisper.lock().await;

    Ok(WhisperStatus {
        enabled: settings.whisper_enabled,
        binary_exists: crate::whisper::WhisperManager::binary_exists(&state.data_dir),
        model_path: settings.whisper_model_path.clone(),
        running: whisper.is_running(),
        port: settings.whisper_port,
    })
}

// ── Server lifecycle ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_whisper_server(state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();

    let model_path = settings
        .whisper_model_path
        .as_deref()
        .ok_or("No Whisper model configured. Download a model first.")?
        .to_string();

    state
        .whisper
        .lock()
        .await
        .start(&model_path, settings.whisper_port, &state.data_dir)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_whisper_server(state: State<'_, AppState>) -> Result<(), String> {
    state.whisper.lock().await.stop();
    Ok(())
}

// ── Transcription ─────────────────────────────────────────────────────────────

/// Transcribe audio.
/// `audio_data` is a Vec<u8> of raw audio bytes (webm or wav).
/// `ext` is the audio extension: "webm" or "wav".
#[tauri::command]
pub async fn transcribe_audio(
    audio_data: Vec<u8>,
    ext: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();

    {
        let mut whisper = state.whisper.lock().await;
        if !whisper.is_running() {
            let model_path = settings
                .whisper_model_path
                .as_deref()
                .ok_or("No Whisper model configured.")?
                .to_string();
            whisper
                .start(&model_path, settings.whisper_port, &state.data_dir)
                .await
                .map_err(|e| format!("Failed to start whisper-server: {}", e))?;
        }
    }

    let language = settings.whisper_language.clone();
    state
        .whisper
        .lock()
        .await
        .transcribe(&audio_data, &ext, &language)
        .await
        .map_err(|e| e.to_string())
}

// ── Downloads ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn download_whisper_binary(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let variant = state.settings.lock().unwrap().whisper_variant.clone();
    // Stop the server and give Windows a moment to fully release file handles
    // before we remove the directory during download.
    state.whisper.lock().await.stop();
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Use an isolated sub-directory so whisper.cpp DLLs never overwrite
    // the llama-server DLLs that live in the parent bin/ folder.
    let bin_dir = state.data_dir.join("bin").join("whisper");
    let (tx, mut rx) = mpsc::channel::<DownloadProgress>(32);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(progress) = rx.recv().await {
            let _ = app_clone.emit("server_binary_progress", &progress);
        }
    });

    downloader::download_whisper_binary(&bin_dir, &variant, tx)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_whisper_model(
    size: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let models_dir = state.data_dir.join("whisper-models");
    let (tx, mut rx) = mpsc::channel::<DownloadProgress>(32);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(progress) = rx.recv().await {
            let _ = app_clone.emit("download_progress", &progress);
        }
    });

    let dest = downloader::download_whisper_model(&size, &models_dir, tx)
        .await
        .map_err(|e| e.to_string())?;

    Ok(dest.to_string_lossy().to_string())
}
