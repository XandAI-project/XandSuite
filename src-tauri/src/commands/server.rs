use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;

use crate::models::DownloadProgress;
use crate::process_ext::HideWindowStd;
use crate::server::{downloader::BinaryVariant, LlamaServerManager};
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GpuInfo {
    pub name: String,
    /// Suggested download variant based on detected GPU
    pub recommended_variant: String,
    pub reason: String,
}

/// Detect the primary GPU and recommend the best llama-server build variant.
#[tauri::command]
pub fn detect_gpu() -> Result<GpuInfo, String> {
    let name = query_gpu_name();

    let (recommended_variant, reason) = recommend_variant(&name);

    Ok(GpuInfo { name, recommended_variant, reason })
}

fn query_gpu_name() -> String {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("powershell");
        cmd.hide_window();
        let output = cmd
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-WmiObject Win32_VideoController | \
                  Where-Object { $_.Name -notlike '*Microsoft*' -and $_.Name -notlike '*Basic*' } | \
                  Sort-Object AdapterRAM -Descending | \
                  Select-Object -First 1 -ExpandProperty Name)",
            ])
            .output();

        match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => String::new(),
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("lspci").output();
        if let Ok(o) = output {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                let l = line.to_lowercase();
                if l.contains("vga") || l.contains("3d controller") || l.contains("display") {
                    // Return just the device description part after ":"
                    if let Some(pos) = line.find(": ") {
                        return line[pos + 2..].to_string();
                    }
                    return line.to_string();
                }
            }
        }
        String::new()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        String::new()
    }
}

fn recommend_variant(gpu_name: &str) -> (String, String) {
    let lower = gpu_name.to_lowercase();

    if lower.is_empty() {
        return (
            "cpu".into(),
            "Could not detect GPU — defaulting to CPU build.".into(),
        );
    }

    let is_nvidia = lower.contains("nvidia")
        || lower.contains("rtx")
        || lower.contains("gtx")
        || lower.contains("quadro")
        || lower.contains("tesla");

    if is_nvidia {
        // RTX 5000 series = Blackwell (GB2xx) — requires CUDA 13
        let is_blackwell = lower.contains("rtx 50")
            || lower.contains("rtx50")
            || lower.contains(" 5060")
            || lower.contains(" 5070")
            || lower.contains(" 5080")
            || lower.contains(" 5090");

        if is_blackwell {
            return (
                "cuda13".into(),
                format!(
                    "Detected NVIDIA Blackwell GPU ({}). \
                     RTX 50 series requires CUDA 13 — CUDA 11/12 do not support this architecture.",
                    gpu_name
                ),
            );
        }

        // RTX 2000 / 3000 / 4000 (Turing / Ampere / Ada Lovelace) → CUDA 12
        // Also covers Quadro / A-series / H100 data center cards.
        // Note: CUDA 11 builds are no longer distributed by llama.cpp;
        //       CUDA 12 runtime is backward-compatible with these GPUs.
        let is_modern_rtx = lower.contains("rtx")
            || lower.contains("a100")
            || lower.contains("a6000")
            || lower.contains("a5000")
            || lower.contains("a4000")
            || lower.contains("h100")
            || lower.contains("h200");
        if is_modern_rtx {
            return (
                "cuda12".into(),
                format!(
                    "Detected NVIDIA {} — CUDA 12 build recommended. \
                     (llama.cpp no longer ships CUDA 11 builds; \
                     CUDA 12 runtime works on all RTX 20/30/40 series cards.)",
                    gpu_name
                ),
            );
        }

        // GTX 10xx / 16xx (Pascal / Turing without RT cores) — CUDA 12 still works
        // but Vulkan is a safe fallback if drivers are old
        let is_gtx = lower.contains("gtx");
        if is_gtx {
            return (
                "cuda12".into(),
                format!(
                    "Detected NVIDIA GTX GPU ({}). CUDA 12 build should work; \
                     use Vulkan if you experience driver issues.",
                    gpu_name
                ),
            );
        }

        // Older / unrecognised NVIDIA
        return (
            "vulkan".into(),
            format!(
                "Detected NVIDIA GPU ({}). Vulkan build recommended for compatibility.",
                gpu_name
            ),
        );
    }

    let is_amd = lower.contains("amd")
        || lower.contains("radeon")
        || lower.contains(" rx ")
        || lower.contains("vega")
        || lower.contains("navi");

    if is_amd {
        return (
            "vulkan".into(),
            format!(
                "Detected AMD GPU ({}). Vulkan build recommended for GPU acceleration.",
                gpu_name
            ),
        );
    }

    let is_intel = lower.contains("intel") && lower.contains("arc");
    if is_intel {
        return (
            "vulkan".into(),
            format!(
                "Detected Intel Arc GPU ({}). Vulkan build recommended.",
                gpu_name
            ),
        );
    }

    (
        "cpu".into(),
        format!(
            "Detected GPU: {}. No GPU-specific build matched — using CPU build.",
            gpu_name
        ),
    )
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerStatus {
    pub running: bool,
    pub port: u16,
    pub model: Option<String>,
    pub binary_exists: bool,
}

/// Get the current status of the internal llama-server.
///
/// If no child process is tracked, this command probes the configured port.
/// When an orphaned server is found (e.g. from a previous app session) it
/// is adopted so the UI shows the correct running state and the engine
/// reconnects automatically.
#[tauri::command]
pub async fn get_server_status(state: State<'_, AppState>) -> Result<ServerStatus, String> {
    let mut server = state.server.lock().await;
    let running = server.is_running();
    let model = server.current_model().map(String::from);

    // No child process tracked — probe the port to detect an orphaned server.
    if !running {
        let (probe_port, last_model) = {
            let s = state.settings.lock().unwrap();
            (s.llama_server_port, s.last_server_model.clone())
        };

        if let Some(detected) = probe_running_server(probe_port, last_model.as_deref()).await {
            log::info!(
                "Detected orphaned llama-server on port {} (model: {}). Adopting.",
                probe_port,
                detected
            );
            server.adopt(probe_port, Some(detected.clone()));

            // Reconnect the engine to the already-running server.
            drop(server);
            let url = format!("http://127.0.0.1:{}", probe_port);
            let _ = state.engine.connect_remote(url, None, None);

            let binary_exists = LlamaServerManager::binary_exists(&state.data_dir);
            return Ok(ServerStatus {
                running: true,
                port: probe_port,
                model: Some(detected),
                binary_exists,
            });
        }
    }

    let port = server.port();
    let binary_exists = LlamaServerManager::binary_exists(&state.data_dir);
    Ok(ServerStatus {
        running,
        port,
        model,
        binary_exists,
    })
}

/// Probe `http://127.0.0.1:{port}/health`. If healthy, try to retrieve the
/// loaded model name from `/v1/models`. Falls back to `last_known_model`.
/// Returns `None` when no server is reachable on that port.
async fn probe_running_server(port: u16, last_known_model: Option<&str>) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;

    // Quick health check — a 200 or 503 ("loading") both mean something is there.
    let health = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .ok()?;

    if !health.status().is_success() && health.status().as_u16() != 503 {
        return None;
    }

    // Try to read the model name from /v1/models (OpenAI-compatible endpoint).
    if let Ok(resp) = client
        .get(format!("http://127.0.0.1:{}/v1/models", port))
        .send()
        .await
    {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(id) = json["data"][0]["id"].as_str() {
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        }
    }

    // Fall back to the last model path saved in settings.
    last_known_model.map(String::from)
}

/// On Linux, scan `bin_dir` for shared-library files (`*.so` / `*.so.*`) that
/// are 0 bytes — a symptom of the old extractor writing empty files instead of
/// real symlinks.  When any are found the entire bin directory is removed so a
/// clean re-download produces correct symlinks.
///
/// Returns `Err` with a user-facing message when corruption is detected.
#[cfg(target_os = "linux")]
fn check_and_purge_corrupt_libs(bin_dir: &std::path::Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(bin_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // bin_dir does not exist yet — nothing to check
    };

    let mut corrupt: Vec<String> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Match libXxx.so and libXxx.so.0 / libXxx.so.0.0.8795 etc.
        if !name.contains(".so") {
            continue;
        }
        // Use symlink_metadata so we see the symlink itself, not what it points to.
        if let Ok(meta) = entry.path().symlink_metadata() {
            if meta.file_type().is_symlink() {
                continue; // Proper symlink — fine.
            }
            if meta.is_file() && meta.len() == 0 {
                corrupt.push(name);
            }
        }
    }

    if corrupt.is_empty() {
        return Ok(());
    }

    log::warn!(
        "Detected {} corrupted shared-library file(s) in {:?}: {:?}. \
         Purging bin directory so a fresh download creates correct symlinks.",
        corrupt.len(), bin_dir, corrupt
    );

    // Remove everything in bin_dir so the re-download starts clean.
    if let Ok(entries) = std::fs::read_dir(bin_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }

    Err(format!(
        "Shared libraries are corrupted ({} zero-byte .so file(s) detected: {}).\n\
         The bin directory has been cleared — please re-download llama-server \
         in the Models tab.",
        corrupt.len(),
        corrupt.join(", ")
    ))
}

/// Start the internal llama-server with the given model file.
/// Automatically connects the engine to the local server on success.
///
/// `mmproj_path` — when provided, overrides `settings.mmproj_path` for this
/// launch only (used when a vision projection file is bundled with the model).
#[tauri::command]
pub async fn start_local_server(
    model_path: String,
    mmproj_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap().clone();
    let data_dir = state.data_dir.clone();
    let port = settings.llama_server_port;

    // Linux only: detect the 0-byte .so files left by the old extractor and
    // force a re-download before trying to spawn a server that will crash.
    #[cfg(target_os = "linux")]
    check_and_purge_corrupt_libs(&data_dir.join("bin"))?;

    // Always update mmproj from the caller's value so stale paths never
    // bleed across models.  Explicitly clearing (None or empty string) removes
    // a previously-persisted projection path, preventing a mismatch crash.
    match mmproj_path.as_deref() {
        Some(mp) if !mp.is_empty() => {
            log::info!("start_local_server: using mmproj: {}", mp);
            settings.mmproj_path = Some(mp.to_string());
        }
        _ => {
            if settings.mmproj_path.is_some() {
                log::info!("start_local_server: clearing stale mmproj (not needed for this model)");
            }
            settings.mmproj_path = None;
        }
    }

    // If an adopted (orphaned) server is running on the port, we must not try
    // to spawn a second process. Clear the adopted state first; the new start()
    // call will attempt to kill our own child (none here) and spawn fresh.
    // If the orphaned process is still occupying the port, start() will fail
    // with a meaningful error so the user knows to stop the existing server.
    {
        let mut server = state.server.lock().await;
        if server.is_adopted() {
            log::info!(
                "Clearing adopted server state before starting a new server on port {}.",
                port
            );
            server.clear_adopted();
        }
    }

    // Start the subprocess (tokio Mutex allows holding across .await)
    {
        let mut server = state.server.lock().await;
        server
            .start(&model_path, &settings, &data_dir)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Persist the last-used model path and the resolved mmproj (or clear it).
    {
        let mut s = state.settings.lock().unwrap();
        s.last_server_model = Some(model_path.clone());
        // Mirror the same clear-or-set logic so the persisted settings always
        // reflect what was actually launched.
        match mmproj_path.as_deref() {
            Some(mp) if !mp.is_empty() => s.mmproj_path = Some(mp.to_string()),
            _ => s.mmproj_path = None,
        }
        let json = serde_json::to_string(&*s).map_err(|e| e.to_string())?;
        let db = state.db.lock().unwrap();
        db.set_setting("app_settings", &json).map_err(|e| e.to_string())?;
    }

    // Connect the engine to the now-running local server
    let server_url = format!("http://127.0.0.1:{}", port);
    state
        .engine
        .connect_remote(server_url, None, None)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Stop the internal llama-server and disconnect the engine.
#[tauri::command]
pub async fn stop_local_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut server = state.server.lock().await;
    server.stop();
    Ok(())
}

/// Called by the chat pipeline after each completed response to reset the idle timer.
#[tauri::command]
pub async fn touch_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut server = state.server.lock().await;
    server.touch();
    Ok(())
}

/// Ensure the internal server is running before a chat request.
/// If it was auto-stopped due to inactivity, restarts it with the last-used model.
#[tauri::command]
pub async fn ensure_server_running(state: State<'_, AppState>) -> Result<bool, String> {
    let is_running = {
        let mut server = state.server.lock().await;
        server.is_running()
    };

    if is_running {
        return Ok(true);
    }

    // Try to auto-restart with the last known model
    let (last_model, settings, data_dir) = {
        let s = state.settings.lock().unwrap();
        (s.last_server_model.clone(), s.clone(), state.data_dir.clone())
    };

    let Some(model_path) = last_model else {
        return Ok(false); // No model to restart with
    };

    log::info!("Auto-restarting llama-server with model: {}", model_path);

    let port = settings.llama_server_port;
    {
        let mut server = state.server.lock().await;
        server
            .start(&model_path, &settings, &data_dir)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Re-connect engine to the restarted server
    state
        .engine
        .connect_remote(format!("http://127.0.0.1:{}", port), None, None)
        .map_err(|e| e.to_string())?;

    Ok(true)
}

/// Download the llama-server binary from GitHub releases.
/// Emits `server_binary_progress` events with `DownloadProgress` payloads.
///
/// The running server (if any) is stopped before extraction so that Windows
/// does not hold file locks on the DLLs we need to overwrite.
#[tauri::command]
pub async fn download_llama_server(
    app: AppHandle,
    variant: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let bin_dir = state.data_dir.join("bin");
    let bv = BinaryVariant::from_str(&variant);

    // Stop the internal server so Windows releases its locks on every DLL
    // inside bin_dir before we try to overwrite them.
    {
        let mut server = state.server.lock().await;
        if server.is_running() {
            log::info!("Stopping llama-server before binary update…");
            server.stop();
            // Give the OS a moment to release file handles
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }

    let (tx, mut rx) = mpsc::channel::<DownloadProgress>(64);

    // Emit progress events to frontend
    let app_clone = app.clone();
    tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            let _ = app_clone.emit("server_binary_progress", &p);
        }
    });

    crate::server::downloader::download_llama_server(bv, &bin_dir, tx)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}
