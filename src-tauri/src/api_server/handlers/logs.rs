use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::Value;
use std::sync::Arc;

use crate::state::AppState;

pub async fn get_logs(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let buf = state.log_buffer.lock().unwrap();
    let logs: Vec<&Value> = buf.iter().collect();
    Ok(Json(serde_json::json!(logs)))
}
