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
        // AMD APU / integrated graphics (Ryzen built-in Radeon) use the same
        // driver stack as discrete Radeon but Vulkan compute is unreliable on
        // Linux for these parts.  The CPU build is stable and fast enough.
        // Discrete cards (RX 6xxx / RX 7xxx / RX 9xxx, Vega, Navi) do work
        // well with Vulkan on Linux.
        //
        // APU detection heuristics (lspci codename or model-number patterns):
        //   Raphael   — Ryzen 7000 desktop   (Radeon 610M / 700M)
        //   Rembrandt — Ryzen 6000 mobile     (Radeon 680M)
        //   Phoenix   — Ryzen 7040 mobile     (Radeon 780M)
        //   Hawk Point — Ryzen 8040 mobile
        //   Strix Point — Ryzen AI 300
        //   Mendocino — budget Ryzen 7020
        //   Model numbers ending in 'M' (mobile/integrated suffix)
        #[cfg(target_os = "linux")]
        {
            let is_apu = lower.contains("raphael")
                || lower.contains("rembrandt")
                || lower.contains("phoenix")
                || lower.contains("hawk point")
                || lower.contains("strix")
                || lower.contains("mendocino")
                || lower.contains("integrated")
                // Integrated Radeon model numbers: 610M, 680M, 700M, 740M,
                // 760M, 780M, 890M, etc.  Match "radeon NNNm" patterns.
                || {
                    let apu_models = ["610m","660m","680m","700m","740m",
                                      "760m","780m","890m","radeon m"];
                    apu_models.iter().any(|m| lower.contains(m))
                };

            if is_apu {
                return (
                    "cpu".into(),
                    format!(
                        "Detected AMD integrated/APU graphics ({}).\n\
                         Vulkan compute is unreliable on these GPUs under Linux — \
                         CPU build recommended.",
                        gpu_name
                    ),
                );
            }
        }

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

/// On Linux, scan `bin_dir` for shared-library files that are 0-byte regular
/// files — a symptom of the old extractor writing empty placeholder files
/// instead of real OS symlinks.
///
/// **Repair strategy (preferred):**
/// Each corrupt file `libXXX.so.N` should be a symlink pointing to the next
/// link in the versioned chain (e.g. `libXXX.so.0` → `libXXX.so.0.0.8795`).
/// We find the real non-zero target already on disk and replace the empty file
/// with a proper symlink — no re-download required.
///
/// **Fallback:** if a corrupt file has no resolvable target (the versioned
/// binary is also missing), the entire bin directory is purged so the next
/// download starts clean, and an `Err` is returned asking the user to
/// re-download.
#[cfg(target_os = "linux")]
fn check_and_repair_libs(bin_dir: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs as unix_fs;

    let read_dir = |p: &std::path::Path| -> Vec<(String, std::path::PathBuf, std::fs::Metadata)> {
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let meta = e.path().symlink_metadata().ok()?;
                Some((e.file_name().to_string_lossy().into_owned(), e.path(), meta))
            })
            .collect()
    };

    let all = read_dir(bin_dir);

    // Partition into: real non-zero .so binaries, 0-byte corrupt .so files,
    // and already-correct symlinks (which we leave untouched).
    let mut real_targets: Vec<String> = Vec::new();
    let mut corrupt:      Vec<String> = Vec::new();

    for (name, _, meta) in &all {
        if !name.contains(".so") { continue; }
        if meta.file_type().is_symlink() { continue; } // already correct
        if meta.is_file() {
            if meta.len() > 0 {
                real_targets.push(name.clone());
            } else {
                corrupt.push(name.clone());
            }
        }
    }

    if corrupt.is_empty() { return Ok(()); }

    log::warn!(
        "Detected {} corrupted (0-byte) shared-library file(s) in {:?}: {:?}",
        corrupt.len(), bin_dir, corrupt
    );

    // --- Pass 1: link each corrupt name to a real (non-zero) versioned file ---
    // e.g. "libmtmd.so.0" (0-byte) → "libmtmd.so.0.0.8795" (real binary).
    // A valid target must START WITH `corrupt_name + "."` (more version digits).
    let mut still_corrupt: Vec<String> = Vec::new();

    for name in &corrupt {
        let prefix = format!("{}.", name); // e.g. "libmtmd.so.0."
        let target = real_targets.iter().find(|t| t.starts_with(&prefix));

        if let Some(t) = target {
            let link_path = bin_dir.join(name);
            let _ = std::fs::remove_file(&link_path);
            match unix_fs::symlink(t, &link_path) {
                Ok(_) => log::info!("Repaired symlink: {} -> {}", name, t),
                Err(e) => {
                    log::warn!("Could not create symlink {} -> {}: {}", name, t, e);
                    still_corrupt.push(name.clone());
                }
            }
        } else {
            still_corrupt.push(name.clone());
        }
    }

    // --- Pass 2: link names whose target is itself a (now-repaired) symlink ---
    // e.g. "libmtmd.so" (0-byte) → "libmtmd.so.0" (just repaired to a symlink).
    let mut unresolved: Vec<String> = Vec::new();

    for name in still_corrupt {
        let prefix = format!("{}.", name);
        // Re-read the directory so we see the symlinks created in pass 1.
        let refreshed = read_dir(bin_dir);
        let target = refreshed.iter().find(|(n, _, m)| {
            n.starts_with(&prefix) && (m.file_type().is_symlink() || (m.is_file() && m.len() > 0))
        });

        if let Some((t, _, _)) = target {
            let link_path = bin_dir.join(&name);
            let _ = std::fs::remove_file(&link_path);
            match unix_fs::symlink(t, &link_path) {
                Ok(_) => log::info!("Repaired symlink (pass 2): {} -> {}", name, t),
                Err(e) => {
                    log::warn!("Could not create symlink {} -> {}: {}", name, t, e);
                    unresolved.push(name);
                }
            }
        } else {
            unresolved.push(name);
        }
    }

    if unresolved.is_empty() {
        log::info!(
            "All shared-library symlinks in {:?} have been repaired in-place.",
            bin_dir
        );
        return Ok(());
    }

    // Some files have no resolvable target — the versioned binary is missing.
    // Purge so the next download starts completely clean.
    log::warn!(
        "Could not repair {} file(s) (no versioned target on disk): {:?}. \
         Purging bin directory.",
        unresolved.len(), unresolved
    );
    for (_, path, _) in read_dir(bin_dir) {
        let _ = std::fs::remove_file(path);
    }

    Err(format!(
        "Shared libraries are corrupted and could not be repaired automatically \
         ({} file(s) with no versioned target: {}).\n\
         The bin directory has been cleared — please re-download llama-server \
         in the Models tab.",
        unresolved.len(),
        unresolved.join(", ")
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

    // Linux only: detect 0-byte .so files left by the old extractor.
    // Attempts in-place symlink repair first; only purges if repair fails.
    #[cfg(target_os = "linux")]
    check_and_repair_libs(&data_dir.join("bin"))?;

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
