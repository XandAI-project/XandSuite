/// Installer download endpoint.
///
/// Serves pre-built installer binaries from the `version/executables/<version>/`
/// directory so users can download XandSuite directly from a running instance.

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::path::PathBuf;
use tokio_util::io::ReaderStream;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct InstallerInfo {
    pub filename: String,
    pub platform: String,
    pub size_bytes: u64,
    pub download_url: String,
}

#[derive(Debug, Serialize)]
pub struct AvailableInstallers {
    pub version: String,
    pub installers: Vec<InstallerInfo>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn detect_platform(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".exe") || lower.ends_with(".msi") {
        "windows"
    } else if lower.ends_with(".dmg") {
        "macos"
    } else if lower.ends_with(".appimage") || lower.ends_with(".deb") {
        "linux"
    } else {
        "unknown"
    }
}

/// Resolve the directory containing installer binaries.
/// Checks (in order):
///   1. `XANDSUITE_INSTALLERS_DIR` env var
///   2. `version/executables/<version>/` relative to the project root
///   3. `installers/` adjacent to the running binary
fn resolve_installers_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XANDSUITE_INSTALLERS_DIR") {
        return PathBuf::from(dir);
    }

    // In dev: project root has version/executables/<version>/
    if let Ok(exe) = std::env::current_exe() {
        // Binary is at src-tauri/target/{debug,release}/xandsuite
        // Project root is 3 levels up
        if let Some(project_root) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            let versioned = project_root
                .join("version")
                .join("executables")
                .join(APP_VERSION);
            if versioned.exists() {
                return versioned;
            }
        }

        // Adjacent to binary: installers/
        let adjacent = exe.parent().unwrap_or(&exe).join("installers");
        if adjacent.exists() {
            return adjacent;
        }
    }

    PathBuf::from("installers")
}

fn list_installer_files() -> Vec<(String, u64, String)> {
    let dir = resolve_installers_dir();
    if !dir.exists() {
        return vec![];
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let platform = detect_platform(&filename);
            if platform == "unknown" {
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            files.push((filename, size, platform.to_string()));
        }
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /api/download — list available installer binaries
pub async fn list_installers() -> Result<Json<AvailableInstallers>, (StatusCode, String)> {
    let files = list_installer_files();
    let installers: Vec<InstallerInfo> = files
        .into_iter()
        .map(|(filename, size_bytes, platform)| InstallerInfo {
            download_url: format!(
                "/api/download/{}",
                urlencoding::encode(&filename)
            ),
            filename,
            platform,
            size_bytes,
        })
        .collect();

    Ok(Json(AvailableInstallers {
        version: APP_VERSION.to_string(),
        installers,
    }))
}

/// GET /api/download/auto — auto-detect platform from User-Agent and redirect
pub async fn download_auto(
    headers: axum::http::HeaderMap,
) -> Response {
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let target_platform = if ua.contains("windows") || ua.contains("win64") || ua.contains("win32") {
        "windows"
    } else if ua.contains("mac") || ua.contains("darwin") {
        "macos"
    } else if ua.contains("linux") {
        "linux"
    } else {
        "windows" // default
    };

    let files = list_installer_files();
    let matched = files
        .iter()
        .find(|(_, _, platform)| platform == target_platform);

    match matched {
        Some((filename, _, _)) => {
            let url = format!("/api/download/{}", urlencoding::encode(filename));
            axum::response::Redirect::temporary(&url).into_response()
        }
        None => {
            (StatusCode::NOT_FOUND, format!(
                "No installer available for platform '{}'. Available: {:?}",
                target_platform,
                files.iter().map(|(f, _, p)| format!("{} ({})", f, p)).collect::<Vec<_>>()
            )).into_response()
        }
    }
}

/// GET /api/download/:filename — serve a specific installer binary
pub async fn download_file(
    axum::extract::Path(filename): axum::extract::Path<String>,
) -> Response {
    // Prevent path traversal
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return (StatusCode::BAD_REQUEST, "Invalid filename").into_response();
    }

    let dir = resolve_installers_dir();
    let file_path = dir.join(&filename);

    if !file_path.exists() || !file_path.is_file() {
        return (StatusCode::NOT_FOUND, format!("Installer '{}' not found", filename)).into_response();
    }

    let file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Cannot open file: {}", e)).into_response();
        }
    };

    let metadata = file_path.metadata().unwrap_or_else(|_| {
        std::fs::metadata(&file_path).unwrap()
    });

    let content_type = if filename.ends_with(".exe") || filename.ends_with(".msi") {
        "application/x-msdownload"
    } else if filename.ends_with(".dmg") {
        "application/x-apple-diskimage"
    } else if filename.ends_with(".AppImage") {
        "application/x-executable"
    } else {
        "application/octet-stream"
    };

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .header(header::CONTENT_LENGTH, metadata.len())
        .body(body)
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response())
}
