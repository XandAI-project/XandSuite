use axum::{extract::State, http::StatusCode, Json};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::state::AppState;

pub async fn stop_generation(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state.generation_cancelled.store(true, Ordering::Relaxed);
    log::info!("[chat] Generation stop requested via HTTP");
    Ok(Json(serde_json::json!({ "success": true })))
}
