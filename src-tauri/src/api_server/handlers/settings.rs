use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::Value;
use std::sync::Arc;

use crate::models::AppSettings;
use crate::state::AppState;

pub async fn get_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AppSettings>, (StatusCode, String)> {
    let s = state.settings.lock().unwrap().clone();
    Ok(Json(s))
}

pub async fn save_settings(
    State(state): State<Arc<AppState>>,
    Json(mut new_settings): Json<AppSettings>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::commands::settings::normalize_settings(&mut new_settings);
    let json = serde_json::to_string(&new_settings)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    {
        let db = state.db.lock().unwrap();
        db.conn
            .execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('app_settings', ?1)",
                rusqlite::params![json],
            )
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    *state.settings.lock().unwrap() = new_settings;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn get_data_dir(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let dir = state.data_dir.to_string_lossy().to_string();
    Ok(Json(serde_json::json!({ "data_dir": dir })))
}
