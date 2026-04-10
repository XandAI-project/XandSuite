use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::commands::packages::{CustomPackage, OfficialPackage};
use crate::state::AppState;

// ── Official packages ─────────────────────────────────────────────────────────

pub async fn list_official_packages(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<OfficialPackage>>, (StatusCode, String)> {
    crate::commands::packages::list_official_packages_inner(&state)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[derive(Deserialize)]
pub struct InstallPackageBody {
    #[serde(default)]
    pub config: HashMap<String, String>,
}

pub async fn install_package(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<InstallPackageBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::commands::packages::install_package_inner(&id, body.config, &state)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn uninstall_package(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::commands::packages::uninstall_package_inner(&id, &state)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Custom packages ───────────────────────────────────────────────────────────

pub async fn list_custom_packages(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<CustomPackage>>, (StatusCode, String)> {
    crate::commands::packages::list_custom_packages_inner(&state)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[derive(Deserialize)]
pub struct SaveCustomPackageBody {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub requirements: String,
    pub code: String,
}

pub async fn save_custom_package(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveCustomPackageBody>,
) -> Result<Json<CustomPackage>, (StatusCode, String)> {
    crate::commands::packages::save_custom_package_inner(
        body.id, body.name, body.description, body.requirements, body.code, &state,
    )
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn get_custom_package_code(
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::commands::packages::get_custom_package_code(id)
        .map(|code| Json(serde_json::json!({ "code": code })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn install_custom_package(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::commands::packages::install_custom_package_inner(&id, &state)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn uninstall_custom_package(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::commands::packages::uninstall_custom_package_inner(&id, &state)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn delete_custom_package(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    crate::commands::packages::delete_custom_package_inner(&id, &state)
        .await
        .map(|_| Json(serde_json::json!({ "success": true })))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}
