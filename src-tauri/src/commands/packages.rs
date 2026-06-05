/// Package Manager commands
///
/// Packages are Python FastMCP stdio servers stored under tools/packages/.
/// Official packages ship with XandSuite and are described by
/// tools/packages/registry.json.  Custom packages are user-written scripts
/// saved to tools/packages/custom/ with metadata kept in SQLite.
///
/// All required Python dependencies are automatically installed via pip
/// before every connect attempt.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{Emitter, State};

use crate::process_ext::HideWindowTokio;

use crate::skills::{McpServerConfig, McpTransport};
use crate::state::AppState;

// ── Constants ─────────────────────────────────────────────────────────────────

const PKG_SERVER_PREFIX: &str = "pkg__";

// ── Logging helpers ───────────────────────────────────────────────────────────

/// Emit a structured log entry to the frontend Logs tab via the `app_log` event.
fn emit_log(app: &tauri::AppHandle, level: &str, message: &str) {
    let _ = app.emit(
        "app_log",
        json!({
            "level": level,
            "message": message,
            "ts": chrono::Utc::now().to_rfc3339(),
        }),
    );
    // Mirror to the native Rust log as well so it appears in the terminal.
    match level {
        "error" => log::error!("{}", message),
        "warn"  => log::warn!("{}", message),
        "debug" => log::debug!("{}", message),
        _       => log::info!("{}", message),
    }
}

/// Format the full anyhow error chain so no cause is hidden.
///
/// Example output:
///   Failed to connect to MCP server 'Jellyfin'
///   → Python process wrote to stderr:
///       ModuleNotFoundError: No module named 'mcp'
///   → MCP stdio subprocess closed its stdout
fn format_error(e: &anyhow::Error) -> String {
    let parts: Vec<String> = e.chain().map(|c| c.to_string()).collect();
    if parts.len() == 1 {
        parts[0].clone()
    } else {
        let mut out = parts[0].clone();
        for cause in &parts[1..] {
            out.push_str("\n  → ");
            out.push_str(cause);
        }
        out
    }
}

// ── Public output types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageArgSchema {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub required: bool,
    #[serde(default)]
    pub placeholder: String,
    pub arg_prefix: String,
    /// For dynamic_select fields: which sibling field provides the base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<String>,
    /// For dynamic_select fields: URL path appended to the depends_on value to fetch options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_endpoint: Option<String>,
    /// For file fields: allowed file extensions passed to the OS file picker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_extensions: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialPackage {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub icon: String,
    pub script: String,
    pub requires: Vec<String>,
    pub args_schema: Vec<PackageArgSchema>,
    pub installed: bool,
    pub config: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPackage {
    pub id: String,
    pub name: String,
    pub description: String,
    /// requirements.txt-style list of pip dependencies (newline-separated).
    pub requirements: String,
    pub created_at: String,
    pub installed: bool,
}

// ── Internal storage types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledPackage {
    package_id: String,
    mcp_server_id: String,
    config: HashMap<String, String>,
    installed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCustomPackage {
    id: String,
    name: String,
    description: String,
    #[serde(default)]
    requirements: String,
    created_at: String,
}

// ── Path helpers ──────────────────────────────────────────────────────────────

fn resolve_tools_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XANDSUITE_TOOLS_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe_dir = exe.parent().unwrap_or(&exe);

        // Bundled install: tools/ sits next to the binary
        let candidate = exe_dir.join("tools");
        if candidate.exists() {
            return candidate;
        }

        // Some Tauri bundles place resources one level up (e.g. macOS .app)
        if let Some(parent) = exe_dir.parent() {
            let candidate = parent.join("resources").join("tools");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // Dev mode: CARGO_MANIFEST_DIR is src-tauri/, tools/ is one level up
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(manifest)
        .parent()
        .unwrap_or(&PathBuf::from("."))
        .join("tools")
}

fn packages_dir() -> PathBuf {
    resolve_tools_dir().join("packages")
}

/// Resolve `${TOOLS_DIR}/packages/official/foo.py` to a proper OS PathBuf.
fn official_script_path(script_template: &str) -> PathBuf {
    let tools_dir = resolve_tools_dir();
    let relative = script_template
        .strip_prefix("${TOOLS_DIR}/")
        .or_else(|| script_template.strip_prefix("${TOOLS_DIR}\\"))
        .unwrap_or(script_template);
    relative.split('/').fold(tools_dir, |acc, c| acc.join(c))
}

// ── DB helpers ────────────────────────────────────────────────────────────────

fn load_installed(state: &AppState) -> Vec<InstalledPackage> {
    let db = state.db.lock().unwrap();
    db.get_setting("installed_packages")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_installed(state: &AppState, packages: &[InstalledPackage]) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    let json = serde_json::to_string(packages).map_err(|e| e.to_string())?;
    db.set_setting("installed_packages", &json).map_err(|e| e.to_string())
}

fn load_custom_meta(state: &AppState) -> Vec<StoredCustomPackage> {
    let db = state.db.lock().unwrap();
    db.get_setting("custom_packages")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_custom_meta(state: &AppState, packages: &[StoredCustomPackage]) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    let json = serde_json::to_string(packages).map_err(|e| e.to_string())?;
    db.set_setting("custom_packages", &json).map_err(|e| e.to_string())
}

fn mcp_id_for(package_id: &str) -> String {
    format!("{}{}", PKG_SERVER_PREFIX, package_id)
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ── pip auto-install ──────────────────────────────────────────────────────────

/// Parse a requirements.txt-style string into individual package specifiers.
/// Handles blank lines, inline `#` comments, and comma-separated entries.
fn parse_requirements(requirements: &str) -> Vec<String> {
    requirements
        .lines()
        .flat_map(|line| line.split(','))
        .map(|item| {
            let item = if let Some(idx) = item.find('#') { &item[..idx] } else { item };
            item.trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Outcome of a single `pip install` invocation.
enum PipRun {
    /// Install succeeded.
    Ok,
    /// Interpreter binary was not found on PATH.
    InterpreterMissing,
    /// pip ran but exited non-zero. Carries the combined stdout+stderr.
    Failed(String),
}

/// Run `<interpreter> -m pip install [extra_flags] <pkgs>` once.
async fn run_pip_once(interpreter: &str, pkgs: &[String], extra_flags: &[&str]) -> Result<PipRun, String> {
    let mut cmd = tokio::process::Command::new(interpreter);
    cmd.hide_window();
    cmd.args(["-m", "pip", "install", "--quiet", "--no-warn-script-location"]);
    cmd.args(extra_flags);
    cmd.args(pkgs);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = tokio::time::timeout(
        tokio::time::Duration::from_secs(180),
        cmd.output(),
    )
    .await;

    match output {
        Err(_) => Err(format!(
            "pip install timed out after 3 minutes while running '{} -m pip install'",
            interpreter
        )),
        Ok(Err(_)) => Ok(PipRun::InterpreterMissing),
        Ok(Ok(out)) if out.status.success() => Ok(PipRun::Ok),
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            Ok(PipRun::Failed(format!(
                "exit {}:\n{}{}",
                out.status.code().unwrap_or(-1),
                stdout,
                stderr
            )))
        }
    }
}

/// Install Python packages with `python -m pip install`.
/// Falls back to `python3 -m pip install` if `python` is not found.
/// The `mcp` package (required by all FastMCP scripts) is always included.
///
/// On PEP 668 "externally-managed-environment" systems (modern Debian/Ubuntu,
/// Python ≥ 3.11), a plain `pip install` into the system interpreter is blocked.
/// We detect that specific failure and transparently retry into the per-user
/// site-packages (`pip install --user --break-system-packages`). That keeps the
/// install user-scoped (writes to `~/.local`, never touches the OS Python) while
/// still being importable by the same system `python` we launch package scripts
/// with.
async fn pip_install_packages(extra: &[String]) -> Result<(), String> {
    // Always ensure mcp is present; merge with caller-supplied packages.
    let mut pkgs: Vec<String> = vec!["mcp".to_string()];
    for p in extra {
        if !pkgs.iter().any(|x| x.eq_ignore_ascii_case(p)) {
            pkgs.push(p.clone());
        }
    }

    log::info!("[packages] pip install: {}", pkgs.join(", "));

    // Try `python` first, then `python3`.
    for interpreter in &["python", "python3"] {
        match run_pip_once(interpreter, &pkgs, &[]).await? {
            PipRun::Ok => {
                log::info!("[packages] pip install succeeded via {}", interpreter);
                return Ok(());
            }
            // Interpreter not found — try the next candidate.
            PipRun::InterpreterMissing => continue,
            PipRun::Failed(err) => {
                // PEP 668: retry into the user site with --break-system-packages.
                if is_externally_managed_error(&err) {
                    log::warn!(
                        "[packages] {} is externally managed (PEP 668); retrying with --user --break-system-packages",
                        interpreter
                    );
                    match run_pip_once(
                        interpreter,
                        &pkgs,
                        &["--user", "--break-system-packages"],
                    )
                    .await?
                    {
                        PipRun::Ok => {
                            log::info!(
                                "[packages] pip install succeeded via {} (--user --break-system-packages)",
                                interpreter
                            );
                            return Ok(());
                        }
                        PipRun::InterpreterMissing => continue,
                        PipRun::Failed(retry_err) => {
                            return Err(format!("pip install failed ({})", retry_err));
                        }
                    }
                }
                return Err(format!("pip install failed ({})", err));
            }
        }
    }

    Err("Neither 'python' nor 'python3' found. Please install Python 3 and ensure it is on your PATH.".to_string())
}

/// Detect the PEP 668 externally-managed-environment failure from pip's output.
fn is_externally_managed_error(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("externally-managed-environment")
        || lower.contains("externally managed")
        || lower.contains("break-system-packages")
}

// ── Official packages ─────────────────────────────────────────────────────────

pub fn list_official_packages_inner(state: &AppState) -> Result<Vec<OfficialPackage>, String> {
    list_official_packages_impl(state)
}

#[tauri::command]
pub fn list_official_packages(state: State<'_, AppState>) -> Result<Vec<OfficialPackage>, String> {
    list_official_packages_impl(&state)
}

fn list_official_packages_impl(state: &AppState) -> Result<Vec<OfficialPackage>, String> {
    let registry_path = packages_dir().join("registry.json");
    if !registry_path.exists() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(&registry_path)
        .map_err(|e| format!("Failed to read packages registry: {}", e))?;
    let registry: Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid packages registry JSON: {}", e))?;

    let installed = load_installed(&state);
    let pkgs = registry
        .get("packages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut result = Vec::new();
    for pkg in pkgs {
        let id = pkg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mcp_id = mcp_id_for(&id);
        let installed_entry = installed.iter().find(|i| i.mcp_server_id == mcp_id);
        let config = installed_entry.map(|e| e.config.clone()).unwrap_or_default();
        let args_schema: Vec<PackageArgSchema> = pkg
            .get("args_schema")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let requires: Vec<String> = pkg
            .get("requires")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        result.push(OfficialPackage {
            id,
            name: pkg.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            description: pkg.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            category: pkg.get("category").and_then(|v| v.as_str()).unwrap_or("General").to_string(),
            icon: pkg.get("icon").and_then(|v| v.as_str()).unwrap_or("Package").to_string(),
            script: pkg.get("script").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            requires,
            args_schema,
            installed: installed_entry.is_some(),
            config,
        });
    }
    Ok(result)
}

/// Install an official package:
///  1. Resolve and validate the script path.
///  2. Auto-install pip requirements (mcp + package `requires`).
///  3. Connect as an MCP server.
///  4. Persist the install record.
pub async fn install_package_inner(
    package_id: &str,
    config: HashMap<String, String>,
    state: &AppState,
) -> Result<(), String> {
    install_package_impl(package_id.to_string(), config, state).await
}

#[tauri::command]
pub async fn install_package(
    package_id: String,
    config: HashMap<String, String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    install_package_impl(package_id, config, &state).await
}

async fn install_package_impl(
    package_id: String,
    config: HashMap<String, String>,
    state: &AppState,
) -> Result<(), String> {
    let registry_path = packages_dir().join("registry.json");
    let text = std::fs::read_to_string(&registry_path)
        .map_err(|e| format!("Failed to read packages registry: {}", e))?;
    let registry: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;

    let pkg = registry
        .get("packages")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&package_id))
        })
        .ok_or_else(|| format!("Package '{}' not found in registry", package_id))?
        .clone();

    let script_template = pkg.get("script").and_then(|v| v.as_str()).ok_or("Package has no script field")?;
    let script_path = official_script_path(script_template);

    if !script_path.exists() {
        return Err(format!(
            "Script not found at '{}'. Make sure the XandSuite tools directory is intact.",
            script_path.display()
        ));
    }

    let requires: Vec<String> = pkg
        .get("requires")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // ── Auto-install pip dependencies ─────────────────────────────────────────
    emit_log(
        &state.app_handle,
        "info",
        &format!("[packages] Installing pip dependencies for '{}': {}", package_id, requires.join(", ")),
    );
    if let Err(e) = pip_install_packages(&requires).await {
        let msg = format!("[packages] pip install failed for '{}': {}", package_id, e);
        emit_log(&state.app_handle, "error", &msg);
        return Err(msg);
    }

    // ── Build MCP server config ───────────────────────────────────────────────
    let args_schema: Vec<PackageArgSchema> = pkg
        .get("args_schema")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let mut args = vec![script_path.to_string_lossy().to_string()];
    for field in &args_schema {
        if let Some(v) = config.get(&field.name) {
            if !v.is_empty() {
                args.push(field.arg_prefix.clone());
                args.push(v.clone());
            }
        }
    }

    let mcp_id = mcp_id_for(&package_id);
    let cfg = McpServerConfig {
        id: mcp_id.clone(),
        name: pkg.get("name").and_then(|v| v.as_str()).unwrap_or(&package_id).to_string(),
        description: pkg.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        transport: McpTransport::Stdio { command: "python".to_string(), args },
        builtin: false,
        enabled: true,
        icon: pkg.get("icon").and_then(|v| v.as_str()).unwrap_or("Package").to_string(),
    };

    emit_log(
        &state.app_handle,
        "info",
        &format!("[packages] Connecting MCP server for '{}'…", package_id),
    );
    if let Err(e) = state.skills.connect_server(cfg).await {
        let msg = format!(
            "[packages] Failed to connect '{}'\n{}",
            package_id,
            format_error(&e)
        );
        emit_log(&state.app_handle, "error", &msg);
        return Err(msg);
    }

    let mut installed = load_installed(&state);
    installed.retain(|i| i.mcp_server_id != mcp_id);
    installed.push(InstalledPackage {
        package_id: package_id.clone(),
        mcp_server_id: mcp_id,
        config,
        installed_at: now_rfc3339(),
    });
    save_installed(&state, &installed)?;

    emit_log(&state.app_handle, "info", &format!("[packages] '{}' installed successfully", package_id));
    Ok(())
}

pub async fn uninstall_package_inner(package_id: &str, state: &AppState) -> Result<(), String> {
    let mcp_id = mcp_id_for(package_id);
    state.skills.disconnect_server(&mcp_id).await;
    let mut installed = load_installed(state);
    installed.retain(|i| i.mcp_server_id != mcp_id);
    save_installed(state, &installed)?;
    log::info!("[packages] '{}' uninstalled", package_id);
    Ok(())
}

#[tauri::command]
pub async fn uninstall_package(
    package_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    uninstall_package_inner(&package_id, &state).await
}

// ── Custom packages ───────────────────────────────────────────────────────────

pub fn list_custom_packages_inner(state: &AppState) -> Result<Vec<CustomPackage>, String> {
    let meta_list = load_custom_meta(state);
    let installed = load_installed(state);
    Ok(meta_list
        .into_iter()
        .map(|m| {
            let mcp_id = mcp_id_for(&m.id);
            CustomPackage {
                id: m.id,
                name: m.name,
                description: m.description,
                requirements: m.requirements,
                created_at: m.created_at,
                installed: installed.iter().any(|i| i.mcp_server_id == mcp_id),
            }
        })
        .collect())
}

#[tauri::command]
pub fn list_custom_packages(state: State<'_, AppState>) -> Result<Vec<CustomPackage>, String> {
    let meta_list = load_custom_meta(&state);
    let installed = load_installed(&state);
    Ok(meta_list
        .into_iter()
        .map(|m| {
            let mcp_id = mcp_id_for(&m.id);
            CustomPackage {
                id: m.id,
                name: m.name,
                description: m.description,
                requirements: m.requirements,
                created_at: m.created_at,
                installed: installed.iter().any(|i| i.mcp_server_id == mcp_id),
            }
        })
        .collect())
}

pub fn save_custom_package_inner(
    id: String, name: String, description: String, requirements: String, code: String, state: &AppState,
) -> Result<CustomPackage, String> {
    save_custom_package_impl(id, name, description, requirements, code, state)
}

/// Save (create or overwrite) a custom package.
/// Writes `{id}.py` and updates DB metadata.
#[tauri::command]
pub fn save_custom_package(
    id: String,
    name: String,
    description: String,
    requirements: String,
    code: String,
    state: State<'_, AppState>,
) -> Result<CustomPackage, String> {
    save_custom_package_impl(id, name, description, requirements, code, &state)
}

fn save_custom_package_impl(
    id: String, name: String, description: String, requirements: String, code: String, state: &AppState,
) -> Result<CustomPackage, String> {
    if id.chars().any(|c| !c.is_alphanumeric() && c != '_' && c != '-') {
        return Err(
            "Package ID may only contain letters, numbers, underscores, and hyphens.".to_string(),
        );
    }

    let custom_dir = packages_dir().join("custom");
    std::fs::create_dir_all(&custom_dir)
        .map_err(|e| format!("Cannot create custom packages directory: {}", e))?;

    std::fs::write(custom_dir.join(format!("{}.py", id)), &code)
        .map_err(|e| format!("Failed to write script: {}", e))?;

    let created_at = now_rfc3339();
    let mut meta_list = load_custom_meta(&state);

    if let Some(existing) = meta_list.iter_mut().find(|m| m.id == id) {
        existing.name = name.clone();
        existing.description = description.clone();
        existing.requirements = requirements.clone();
    } else {
        meta_list.push(StoredCustomPackage {
            id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            requirements: requirements.clone(),
            created_at: created_at.clone(),
        });
    }
    save_custom_meta(&state, &meta_list)?;

    log::info!("[packages] custom '{}' saved", id);
    Ok(CustomPackage {
        id,
        name,
        description,
        requirements,
        created_at,
        installed: false,
    })
}

/// Return the Python source code of a custom package.
#[tauri::command]
pub fn get_custom_package_code(id: String) -> Result<String, String> {
    let path = packages_dir().join("custom").join(format!("{}.py", id));
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read package code: {}", e))
}

pub async fn delete_custom_package_inner(id: &str, state: &AppState) -> Result<(), String> {
    let mcp_id = mcp_id_for(id);
    state.skills.disconnect_server(&mcp_id).await;
    let mut installed = load_installed(state);
    installed.retain(|i| i.mcp_server_id != mcp_id);
    save_installed(state, &installed)?;
    let mut meta_list = load_custom_meta(state);
    meta_list.retain(|m| m.id != id);
    save_custom_meta(state, &meta_list)?;
    let _ = std::fs::remove_file(packages_dir().join("custom").join(format!("{}.py", id)));
    log::info!("[packages] custom '{}' deleted", id);
    Ok(())
}

/// Delete a custom package: disconnect, remove files and metadata.
#[tauri::command]
pub async fn delete_custom_package(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    delete_custom_package_inner(&id, &state).await
}

pub async fn install_custom_package_inner(id: &str, state: &AppState) -> Result<(), String> {
    install_custom_package_impl(id.to_string(), state).await
}

/// Connect a custom package as an MCP server, after auto-installing its requirements.
#[tauri::command]
pub async fn install_custom_package(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    install_custom_package_impl(id, &state).await
}

async fn install_custom_package_impl(id: String, state: &AppState) -> Result<(), String> {
    let meta_list = load_custom_meta(&state);
    let meta = meta_list
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("Custom package '{}' not found", id))?
        .clone();

    let script_path = packages_dir().join("custom").join(format!("{}.py", id));
    if !script_path.exists() {
        return Err(format!(
            "Script file for custom package '{}' not found at '{}'",
            id,
            script_path.display()
        ));
    }

    // ── Auto-install pip requirements ─────────────────────────────────────────
    let extra = parse_requirements(&meta.requirements);
    emit_log(
        &state.app_handle,
        "info",
        &format!("[packages] Installing pip dependencies for custom '{}': {}", id,
            if extra.is_empty() { "mcp (base)".to_string() } else { extra.join(", ") }),
    );
    if let Err(e) = pip_install_packages(&extra).await {
        let msg = format!("[packages] pip install failed for custom '{}': {}", id, e);
        emit_log(&state.app_handle, "error", &msg);
        return Err(msg);
    }

    // ── Connect ───────────────────────────────────────────────────────────────
    let mcp_id = mcp_id_for(&id);
    let cfg = McpServerConfig {
        id: mcp_id.clone(),
        name: meta.name.clone(),
        description: meta.description.clone(),
        transport: McpTransport::Stdio {
            command: "python".to_string(),
            args: vec![script_path.to_string_lossy().to_string()],
        },
        builtin: false,
        enabled: true,
        icon: "Code".to_string(),
    };

    emit_log(
        &state.app_handle,
        "info",
        &format!("[packages] Connecting MCP server for custom '{}'…", id),
    );
    if let Err(e) = state.skills.connect_server(cfg).await {
        let msg = format!(
            "[packages] Failed to connect custom '{}'\n{}",
            id,
            format_error(&e)
        );
        emit_log(&state.app_handle, "error", &msg);
        return Err(msg);
    }

    let mut installed = load_installed(&state);
    installed.retain(|i| i.mcp_server_id != mcp_id);
    installed.push(InstalledPackage {
        package_id: id.clone(),
        mcp_server_id: mcp_id,
        config: HashMap::new(),
        installed_at: now_rfc3339(),
    });
    save_installed(&state, &installed)?;

    emit_log(&state.app_handle, "info", &format!("[packages] custom '{}' installed successfully", id));
    Ok(())
}

pub async fn uninstall_custom_package_inner(id: &str, state: &AppState) -> Result<(), String> {
    let mcp_id = mcp_id_for(id);
    state.skills.disconnect_server(&mcp_id).await;
    let mut installed = load_installed(state);
    installed.retain(|i| i.mcp_server_id != mcp_id);
    save_installed(state, &installed)?;
    log::info!("[packages] custom '{}' uninstalled", id);
    Ok(())
}

/// Disconnect a custom package.
#[tauri::command]
pub async fn uninstall_custom_package(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    uninstall_custom_package_inner(&id, &state).await
}

/// On app startup: reconnect all previously installed packages.
/// pip is NOT run here — packages should already be installed.
pub async fn reconnect_installed_packages(state: &AppState) {
    let installed = load_installed(state);
    if installed.is_empty() {
        return;
    }

    let registry_path = packages_dir().join("registry.json");
    let registry_text = std::fs::read_to_string(&registry_path).unwrap_or_default();
    let registry: Value = serde_json::from_str(&registry_text).unwrap_or(json!({}));
    let pkg_arr: Vec<Value> = registry
        .get("packages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let custom_meta = load_custom_meta(state);

    for record in &installed {
        let pid = &record.package_id;

        if let Some(pkg) = pkg_arr
            .iter()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(pid))
        {
            let script_template = pkg.get("script").and_then(|v| v.as_str()).unwrap_or_default();
            let script_path = official_script_path(script_template);
            if !script_path.exists() {
                log::warn!("[packages] script for '{}' missing, skipping reconnect", pid);
                continue;
            }
            let args_schema: Vec<PackageArgSchema> = pkg
                .get("args_schema")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let mut args = vec![script_path.to_string_lossy().to_string()];
            for field in &args_schema {
                if let Some(v) = record.config.get(&field.name) {
                    if !v.is_empty() {
                        args.push(field.arg_prefix.clone());
                        args.push(v.clone());
                    }
                }
            }
            let cfg = McpServerConfig {
                id: record.mcp_server_id.clone(),
                name: pkg.get("name").and_then(|v| v.as_str()).unwrap_or(pid).to_string(),
                description: pkg.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                transport: McpTransport::Stdio { command: "python".to_string(), args },
                builtin: false,
                enabled: true,
                icon: pkg.get("icon").and_then(|v| v.as_str()).unwrap_or("Package").to_string(),
            };
            if let Err(e) = state.skills.connect_server(cfg).await {
                log::warn!("[packages] failed to reconnect '{}': {}", pid, e);
            }
        } else if let Some(meta) = custom_meta.iter().find(|m| &m.id == pid) {
            let script_path = packages_dir().join("custom").join(format!("{}.py", pid));
            if !script_path.exists() {
                log::warn!("[packages] custom '{}' script missing, skipping", pid);
                continue;
            }
            let cfg = McpServerConfig {
                id: record.mcp_server_id.clone(),
                name: meta.name.clone(),
                description: meta.description.clone(),
                transport: McpTransport::Stdio {
                    command: "python".to_string(),
                    args: vec![script_path.to_string_lossy().to_string()],
                },
                builtin: false,
                enabled: true,
                icon: "Code".to_string(),
            };
            if let Err(e) = state.skills.connect_server(cfg).await {
                log::warn!("[packages] failed to reconnect custom '{}': {}", pid, e);
            }
        }
    }
}

/// Fetch the list of workflow filenames saved on a ComfyUI server.
///
/// Calls `GET {base_url}/userdata?dir=workflows&recurse=true&split_dir=false`
/// and returns the array of filename strings (e.g. `["my_video.json", ...]`).
/// Runs on the Rust side to avoid CORS/CSP restrictions in the WebView.
#[tauri::command]
pub async fn fetch_comfyui_workflows(base_url: String) -> Result<Vec<String>, String> {
    let url = format!(
        "{}/userdata?dir=workflows&recurse=true&split_dir=false",
        base_url.trim_end_matches('/')
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Cannot reach ComfyUI at {base_url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "ComfyUI returned HTTP {}: {}",
            resp.status(),
            url
        ));
    }

    let names: Vec<String> = resp
        .json()
        .await
        .map_err(|e| format!("Invalid JSON from ComfyUI: {e}"))?;

    Ok(names)
}
