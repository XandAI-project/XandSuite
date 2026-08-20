use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;

use crate::models::{DownloadProgress, HfModel};
use crate::state::AppState;

// chrono is available via the crate-level dependency

/// Resolve the models directory from settings.
///
/// If `models_directory` is an absolute path it is used as-is.
/// If it is relative (or the legacy default `"models"`), it is joined with
/// `data_dir`.  An empty string falls back to `data_dir/models`.
pub fn resolve_models_dir(data_dir: &Path, models_directory: &str) -> PathBuf {
    let p = Path::new(models_directory);
    if p.is_absolute() {
        p.to_path_buf()
    } else if models_directory.is_empty() {
        data_dir.join("models")
    } else {
        data_dir.join(models_directory)
    }
}

#[tauri::command]
pub async fn list_hf_models(
    search: Option<String>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<HfModel>, String> {
    let cache_dir = state.data_dir.join("cache");
    let api_token = state.settings.lock().unwrap().hf_api_token.clone();
    let scraper = crate::hf::HfScraper::new(api_token.clone());
    let lim = limit.unwrap_or(50);

    // When searching, always hit the API for accurate results
    let use_cache = search.is_none();

    if use_cache {
        if let Ok(Some(cache)) = scraper.load_cache(&cache_dir).await {
            // Invalidate cache if models have no gguf_files (stale cache from
            // before full=true was added) or if cache is older than 6 hours
            let cache_valid = cache.models.iter().any(|m| !m.gguf_files.is_empty());
            let cache_fresh = chrono::DateTime::parse_from_rfc3339(&cache.last_updated)
                .map(|t| {
                    let age = chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc));
                    age.num_hours() < 6
                })
                .unwrap_or(false);

            if cache_valid && cache_fresh {
                let mut models = cache.models;
                let models_dir = {
                    let dir = state.settings.lock().unwrap().models_directory.clone();
                    resolve_models_dir(&state.data_dir, &dir)
                };
                let downloader = crate::hf::HfDownloader::new(models_dir, api_token);
                for model in &mut models {
                    model.is_downloaded = model.gguf_files.iter().any(|f| {
                        downloader.is_downloaded(&model.id, &f.filename)
                    });
                }
                return Ok(models.into_iter().take(lim as usize).collect());
            }
        }
    }

    let fetch_result = scraper
        .fetch_gguf_models(lim, search.as_deref())
        .await;

    match fetch_result {
        Ok(models) => {
            // Only save cache for non-search requests
            if search.is_none() {
                let _ = scraper.save_cache(&cache_dir, &models).await;
            }
            Ok(models)
        }
        Err(e) => {
            let msg = e.to_string();
            // On rate-limit, fall back to any cached data (even if stale)
            if msg.contains("429") || msg.contains("rate limit") {
                if let Ok(Some(cache)) = scraper.load_cache(&cache_dir).await {
                    if !cache.models.is_empty() {
                        log::warn!("[models] HF API rate-limited; serving stale cache from {}", cache.last_updated);
                        let models_dir = {
                            let dir = state.settings.lock().unwrap().models_directory.clone();
                            resolve_models_dir(&state.data_dir, &dir)
                        };
                        let downloader = crate::hf::HfDownloader::new(models_dir, api_token);
                        let mut models = cache.models;
                        for model in &mut models {
                            model.is_downloaded = model.gguf_files.iter().any(|f| {
                                downloader.is_downloaded(&model.id, &f.filename)
                            });
                        }
                        return Ok(models.into_iter().take(lim as usize).collect());
                    }
                }
            }
            Err(msg)
        }
    }
}

#[tauri::command]
pub async fn refresh_hf_models(state: State<'_, AppState>) -> Result<usize, String> {
    let cache_dir = state.data_dir.join("cache");
    let api_token = state.settings.lock().unwrap().hf_api_token.clone();
    let scraper = crate::hf::HfScraper::new(api_token);
    let models = scraper.fetch_gguf_models(100, None).await.map_err(|e| e.to_string())?;
    let count = models.len();
    scraper.save_cache(&cache_dir, &models).await.map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    model_id: String,
    filename: String,
    url: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let (models_dir, api_token) = {
        let s = state.settings.lock().unwrap();
        let dir = resolve_models_dir(&state.data_dir, &s.models_directory);
        (dir, s.hf_api_token.clone())
    };

    let (progress_tx, mut progress_rx) = mpsc::channel::<DownloadProgress>(64);

    let mid = model_id.clone();
    let fname = filename.clone();
    let dl = crate::hf::HfDownloader::new(models_dir, api_token);

    tokio::spawn(async move {
        match dl.download_model(&mid, &fname, &url, progress_tx).await {
            Ok(path) => log::info!("Downloaded model to {:?}", path),
            Err(e) => log::error!("Download failed: {}", e),
        }
    });

    tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            let _ = app.emit("download_progress", &progress);
        }
    });

    Ok(format!("Download started for {}", filename))
}

#[tauri::command]
pub fn list_downloaded_models(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let models_dir = {
        let dir = state.settings.lock().unwrap().models_directory.clone();
        resolve_models_dir(&state.data_dir, &dir)
    };
    let downloader = crate::hf::HfDownloader::new(models_dir, None);
    let downloaded = downloader.list_downloaded_models();

    let result = downloaded
        .into_iter()
        .map(|(model_id, filename)| {
            let path = downloader.get_model_path(&model_id, &filename);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            serde_json::json!({
                "model_id": model_id,
                "filename": filename,
                "path": path.to_string_lossy(),
                "size_bytes": size
            })
        })
        .collect();

    Ok(result)
}

#[tauri::command]
pub async fn delete_model(
    model_id: String,
    filename: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let models_dir = {
        let dir = state.settings.lock().unwrap().models_directory.clone();
        resolve_models_dir(&state.data_dir, &dir)
    };
    let downloader = crate::hf::HfDownloader::new(models_dir, None);
    downloader.delete_model(&model_id, &filename).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_model(model_path: String, state: State<'_, AppState>) -> Result<(), String> {
    state.engine.load_local(model_path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn connect_remote_server(
    server_url: String,
    api_key: Option<String>,
    model_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let (reachable, error) =
        connect_remote_inner(&state, server_url, api_key, model_name).await;
    match error {
        // The probe failed for a concrete reason — surface it to the caller
        // instead of a bare `false`, which the UI could only report as a
        // generic "could not reach server".
        Some(msg) => Err(msg),
        None => Ok(reachable),
    }
}

/// Probe a remote OpenAI-compatible server and install it as the active engine
/// when it answers. Returns `(reachable, error_message)`.
///
/// Shared by the Tauri command and the HTTP handler so both apply the same URL
/// normalization and health check.
pub async fn connect_remote_inner(
    state: &AppState,
    server_url: String,
    api_key: Option<String>,
    model_name: Option<String>,
) -> (bool, Option<String>) {
    let url = crate::engine::remote::normalize_server_url(&server_url);
    if crate::engine::remote::is_unspecified_host(&server_url) {
        log::warn!(
            "Remote server URL '{}' is a wildcard bind address; connecting to {} instead",
            server_url.trim(),
            url
        );
    }

    let probe = crate::engine::remote::RemoteEngine::new(
        url.clone(),
        api_key.clone(),
        model_name.clone(),
    );
    match probe.test_connection().await {
        Ok(true) => {
            if let Err(e) = state.engine.connect_remote(url, api_key, model_name).await {
                return (false, Some(e.to_string()));
            }
            (true, None)
        }
        Ok(false) => (false, None),
        Err(e) => (false, Some(e.to_string())),
    }
}

#[tauri::command]
pub async fn is_engine_loaded(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.engine.is_loaded().await)
}
