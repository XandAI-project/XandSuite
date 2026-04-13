use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tokio::sync::mpsc;

use crate::models::DownloadProgress;
use crate::state::AppState;
use crate::tts::downloader;

// ── Status ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct TtsStatus {
    pub enabled: bool,
    pub models_exist: bool,
    /// Python deps fully installed for the current device (stamp file valid).
    pub deps_ready: bool,
    /// Process is alive (does not mean it has finished loading yet).
    pub running: bool,
    /// Server responded to /health (fully ready to synthesize).
    pub healthy: bool,
    pub port: u16,
    pub voice: String,
    pub speed: f32,
    pub language: String,
    /// Torch device: "cpu", "cuda11", or "cuda12".
    pub device: String,
}

/// Returns server status. Also performs a lightweight /health ping when the
/// process is running so the UI can tell "starting" from "ready".
#[tauri::command]
pub async fn get_tts_status(state: State<'_, AppState>) -> Result<TtsStatus, String> {
    let settings = state.settings.lock().unwrap().clone();
    let mut tts = state.tts.lock().await;
    let running = tts.is_running();
    let healthy = if running { tts.is_healthy().await } else { false };

    Ok(TtsStatus {
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
    })
}

// ── Server lifecycle ──────────────────────────────────────────────────────────

/// Spawn kokoro_server.py in the background and return immediately.
/// The server installs its own Python deps and loads the ONNX model
/// asynchronously — this can take several minutes on first run.
/// Poll `get_tts_status` to track progress (healthy = fully ready).
#[tauri::command]
pub async fn start_tts_server(state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    state
        .tts
        .lock()
        .await
        .spawn(settings.tts_port, &state.data_dir, &settings.tts_device)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_tts_server(state: State<'_, AppState>) -> Result<(), String> {
    state.tts.lock().await.stop();
    Ok(())
}

/// Return the last N lines of the kokoro-server log for diagnostics.
#[tauri::command]
pub async fn get_tts_log(state: State<'_, AppState>) -> Result<String, String> {
    let log = state
        .tts
        .lock()
        .await
        .read_log()
        .unwrap_or_default();

    // Return last 80 lines to keep it manageable
    let lines: Vec<&str> = log.lines().collect();
    let tail = if lines.len() > 80 {
        lines[lines.len() - 80..].join("\n")
    } else {
        log
    };
    Ok(tail)
}

// ── Synthesis ─────────────────────────────────────────────────────────────────

/// Synthesize `text` and return raw WAV bytes.
/// Auto-starts the server if not running and waits for it to become healthy.
#[tauri::command]
pub async fn synthesize_speech(
    text: String,
    voice: String,
    speed: f32,
    state: State<'_, AppState>,
) -> Result<Vec<u8>, String> {
    let settings = state.settings.lock().unwrap().clone();

    {
        let mut tts = state.tts.lock().await;
        if !tts.is_running() {
            // Non-blocking spawn; then wait for health below
            tts.spawn(settings.tts_port, &state.data_dir, &settings.tts_device)
                .map_err(|e| format!("Failed to start TTS server: {}", e))?;
        }
    }

    // Wait for the server to become healthy (up to 10 minutes for first-time
    // dep install + model load)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    loop {
        {
            let mut tts = state.tts.lock().await;
            if tts.is_healthy().await {
                break;
            }
            if !tts.is_running() {
                let log = tts.read_log().unwrap_or_default();
                return Err(format!(
                    "kokoro-server exited before becoming ready.\nLog:\n{}",
                    log.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
                ));
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err("kokoro-server did not become ready within 10 minutes.".into());
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    // Language is derived from the voice name inside the Python server
    // (first character of voice id = KPipeline lang_code).
    state
        .tts
        .lock()
        .await
        .synthesize(&text, &voice, speed)
        .await
        .map_err(|e| e.to_string())
}

// ── Download ──────────────────────────────────────────────────────────────────

/// Download hexgrad/Kokoro-82M into the app-local HF cache for offline use.
/// Delegates to `kokoro_server.py --download`.
/// Emits `download_tts_progress` events while downloading (file = log line).
#[tauri::command]
pub async fn download_tts_models(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    let device = state.settings.lock().unwrap().tts_device.clone();
    let (tx, mut rx) = mpsc::channel::<DownloadProgress>(64);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(progress) = rx.recv().await {
            let _ = app_clone.emit("download_tts_progress", &progress);
        }
    });

    downloader::download_kokoro_models(&data_dir, &device, tx)
        .await
        .map_err(|e| e.to_string())
}

// ── Dependency setup ──────────────────────────────────────────────────────

/// Run `kokoro_server.py --setup` to install Python deps for the current
/// device variant.  Writes a stamp file on success so subsequent server
/// starts skip the dep check entirely.
/// Emits `setup_tts_progress` events with log lines while running.
#[tauri::command]
pub async fn setup_tts_deps(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    let device = state.settings.lock().unwrap().tts_device.clone();

    let log_path = std::env::temp_dir().join("kokoro-setup.log");
    let mut child = crate::tts::KokoroManager::spawn_setup(&data_dir, &device, &log_path)
        .map_err(|e| e.to_string())?;

    let app_clone = app.clone();
    let log_path_clone = log_path.clone();

    tauri::async_runtime::spawn(async move {
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

    Ok(())
}
