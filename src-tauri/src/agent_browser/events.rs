//! Tauri event helpers for the Browser Agent.
//!
//! All emitters are best-effort: a failure is logged but never propagated
//! because the agent loop must keep running even if the frontend is
//! disconnected (e.g. the Browser Agent tab is closed).

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// One decoded screencast frame forwarded to the React canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserAgentFrame {
    pub session_id: String,
    /// Base64-encoded JPEG bytes. Decoded and blit via `ctx.drawImage` on
    /// the frontend; never persisted in Zustand because frames are very hot.
    pub data_base64: String,
    pub width: u32,
    pub height: u32,
    /// Milliseconds since UNIX epoch — used by the UI to measure frame rate.
    pub ts_ms: i64,
}

/// Payload for a confirmation gate (downloads, cross-origin submits, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmRequestPayload {
    pub session_id: String,
    pub conversation_id: String,
    pub request_id: String,
    pub action: String,
    pub target: Option<String>,
    pub rationale: String,
}

pub fn emit_frame(app: &AppHandle, frame: &BrowserAgentFrame) {
    if let Err(e) = app.emit("browser_agent_frame", frame) {
        log::warn!("[browser-agent] emit_frame failed: {}", e);
    }
}

pub fn emit_url(app: &AppHandle, session_id: &str, url: &str) {
    let _ = app.emit(
        "browser_agent_url",
        serde_json::json!({ "session_id": session_id, "url": url }),
    );
}

pub fn emit_title(app: &AppHandle, session_id: &str, title: &str) {
    let _ = app.emit(
        "browser_agent_title",
        serde_json::json!({ "session_id": session_id, "title": title }),
    );
}

pub fn emit_load_state(app: &AppHandle, session_id: &str, state: &str) {
    let _ = app.emit(
        "browser_agent_load_state",
        serde_json::json!({ "session_id": session_id, "state": state }),
    );
}

pub fn emit_confirm_request(app: &AppHandle, payload: &ConfirmRequestPayload) {
    let _ = app.emit("browser_agent_confirm_request", payload);
}

pub fn emit_download(
    app: &AppHandle,
    session_id: &str,
    filename: &str,
    url: &str,
) {
    let _ = app.emit(
        "browser_agent_download",
        serde_json::json!({
            "session_id": session_id,
            "filename": filename,
            "url": url,
        }),
    );
}

/// Emitted after a browser session is launched — either by the user clicking
/// "Start browser" or by the LLM invoking `browser_agent__start_session`.
///
/// The frontend uses the `source` tag to decide whether to auto-start the
/// screencast (LLM-initiated starts still need the viewport to come alive if
/// stealth is off) and to show a subtle toast indicating who launched it.
pub fn emit_session_started(
    app: &AppHandle,
    session_id: &str,
    conversation_id: &str,
    source: &str,
    initial_url: &str,
) {
    let _ = app.emit(
        "browser_agent_session_started",
        serde_json::json!({
            "session_id": session_id,
            "conversation_id": conversation_id,
            "source": source,
            "url": initial_url,
        }),
    );
}
