/// Miscellaneous HTTP handlers for commands that don't fit an existing handler module.
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::state::AppState;

// ── Models ────────────────────────────────────────────────────────────────────

pub async fn get_models_dir(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let dir_setting = state.settings.lock().unwrap().models_directory.clone();
    let resolved = crate::commands::models::resolve_models_dir(&state.data_dir, &dir_setting);
    Ok(Json(serde_json::json!({ "models_dir": resolved.to_string_lossy() })))
}

pub async fn refresh_hf_models(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let cache_dir = state.data_dir.join("cache");
    let api_token = state.settings.lock().unwrap().hf_api_token.clone();
    let scraper = crate::hf::HfScraper::new(api_token);
    let models = scraper
        .fetch_gguf_models(100, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let count = models.len();
    scraper
        .save_cache(&cache_dir, &models)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "count": count })))
}

// ── RAG ───────────────────────────────────────────────────────────────────────

pub async fn reindex_collection(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::commands::rag::reindex_collection_inner(collection_id, &state)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Agents ────────────────────────────────────────────────────────────────────

pub async fn open_task_workspace(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // In web/headless mode, "open in file manager" returns the path instead of opening it.
    let workspace = state.data_dir.join("agent_workspace").join(&task_id);
    Ok(Json(serde_json::json!({ "path": workspace.to_string_lossy() })))
}

// ── ComfyUI ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FetchComfyUiBody {
    pub base_url: String,
}

pub async fn fetch_comfyui_workflows(
    Json(body): Json<FetchComfyUiBody>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    crate::commands::packages::fetch_comfyui_workflows(body.base_url)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── File access ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReadFileBody {
    pub path: String,
}

pub async fn read_file_as_base64(
    Json(body): Json<ReadFileBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let bytes = std::fs::read(&body.path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot read file: {}", e)))?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    Ok(Json(serde_json::json!({ "data": b64 })))
}
