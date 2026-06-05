//! Tauri commands for the Browser Agent tab.
//!
//! The commands are intentionally small: session lifecycle, screencast
//! start/stop, and raw input forwarding. All long-running work (the agent
//! loop itself) still runs through `SkillsExecutor::run` — these commands
//! only manage the Chromium sidecar and the user-interactive side of the
//! viewport (take-over mode).

use std::path::PathBuf;
use tauri::{AppHandle, State};

use crate::agent_browser::controller::BrowserController;
use crate::agent_browser::cookie_vault::{
    self, BrowserCookieSession, CookieEntry, CookieSessionDigest,
};
use crate::agent_browser::events::{self, BrowserAgentFrame};
use crate::agent_browser::profile::ProfileKind;
use crate::agent_browser::session::BrowserSession;
use crate::state::AppState;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Options accepted by [`start_browser_session_core`]. Exposed publicly so
/// the agent-loop dispatch path (`browser_agent__start_session`) can call the
/// same launch routine the Tauri command uses, without duplicating cookie
/// resolution / auto-inject / event emission.
#[derive(Debug, Clone, Default)]
pub struct StartSessionOptions {
    pub profile_name: Option<String>,
    pub initial_url: Option<String>,
    pub chrome_executable: Option<String>,
    pub cookie_session_id: Option<String>,
    /// `"user"` for clicks from the toolbar, `"llm"` when the agent invokes
    /// `browser_agent__start_session`. Used purely for telemetry + the
    /// `browser_agent_session_started` event payload so the UI can show a
    /// tiny attribution hint.
    pub source: String,
}

/// Core implementation shared by the Tauri command
/// [`start_browser_session`] and the agent-callable
/// `browser_agent__start_session` tool. Keeps profile resolution, cookie
/// application, auto-inject, session registry insertion, and event emission
/// in one place so every launch path stays behaviourally identical.
pub async fn start_browser_session_core(
    app: &AppHandle,
    state: &AppState,
    conversation_id: String,
    opts: StartSessionOptions,
) -> Result<serde_json::Value, String> {
    if let Some(existing) = state.browser_sessions.get(&conversation_id).await {
        return Ok(serde_json::json!({
            "session_id": existing.id,
            "conversation_id": conversation_id,
            "reused": true,
            "cookies_applied": 0,
            "source": opts.source,
        }));
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let kind = match opts.profile_name.as_ref() {
        Some(name) if !name.trim().is_empty() => ProfileKind::Named {
            name: name.trim().to_string(),
        },
        _ => ProfileKind::Disposable {
            session_id: session_id.clone(),
        },
    };

    // Resolve cookies BEFORE we launch so the caller gets a clear error if the
    // session id is bogus, instead of a half-launched browser with no auth.
    let cookies_to_apply = match opts.cookie_session_id.as_deref() {
        Some(id) if !id.is_empty() => Some(
            state
                .browser_cookie_vault
                .get(id)
                .ok_or_else(|| format!("cookie session '{}' not found", id))?,
        ),
        _ => None,
    };

    // `BrowserController::launch` consumes `initial_url`, so retain a copy
    // for the post-launch auto-inject pass.
    let start_url_for_inject = opts
        .initial_url
        .as_ref()
        .filter(|u| !u.trim().is_empty())
        .cloned();

    let controller = BrowserController::launch(
        session_id.clone(),
        &state.browser_profiles,
        kind,
        opts.initial_url.clone(),
        opts.chrome_executable.as_ref().map(PathBuf::from),
    )
    .await
    .map_err(|e| format!("failed to launch browser: {}", e))?;

    // Apply cookies (Network.setCookies happens at the browser level so it
    // affects every page in this Chromium, regardless of which page the user
    // navigates to next).
    let mut cookies_applied = 0usize;
    if let Some(record) = cookies_to_apply.as_ref() {
        let params = cookie_vault::to_cookie_params(&record.cookies);
        match controller.set_cookies(params).await {
            Ok(n) => {
                cookies_applied = n;
                log::info!(
                    "[browser-agent] applied {} cookies from session '{}'",
                    n,
                    record.name
                );
            }
            Err(e) => {
                // Don't tear down the browser — the user can still log in
                // manually inside the viewport. Just surface the error.
                log::warn!(
                    "[browser-agent] failed to apply cookie session '{}': {}",
                    record.name,
                    e
                );
            }
        }
    }

    let session = BrowserSession::new(conversation_id.clone(), controller);

    // Mark the user-picked session as "already applied" so the very next
    // navigation doesn't re-send the same cookies via auto-inject.
    if let Some(record) = cookies_to_apply.as_ref() {
        session.mark_cookie_session_applied(&record.id);
    }

    state.browser_sessions.insert(session.clone()).await;

    // If the caller specified a concrete start URL (not about:blank), also
    // run an auto-inject pass for it so any OTHER matching vault sessions —
    // besides the explicitly selected one — get applied as well.
    if let Some(start) = start_url_for_inject {
        let landed = session
            .controller
            .current_url()
            .await
            .unwrap_or(start);
        match session
            .auto_inject_cookies(&landed, &state.browser_cookie_vault)
            .await
        {
            Ok(n) if n > 0 => {
                cookies_applied += n;
                log::info!(
                    "[browser-agent] auto-injected {} additional cookie(s) on launch for {}",
                    n,
                    landed
                );
            }
            Ok(_) => {}
            Err(e) => log::warn!(
                "[browser-agent] auto cookie inject on launch failed for {}: {}",
                landed,
                e
            ),
        }
    }

    // Emit an initial url/title so the toolbar has something to show.
    let initial_url_for_event = session
        .controller
        .current_url()
        .await
        .unwrap_or_else(|_| "about:blank".to_string());
    events::emit_url(app, &session.id, &initial_url_for_event);
    if let Ok(title) = session.controller.current_title().await {
        events::emit_title(app, &session.id, &title);
    }
    events::emit_session_started(
        app,
        &session.id,
        &conversation_id,
        &opts.source,
        &initial_url_for_event,
    );

    Ok(serde_json::json!({
        "session_id": session.id,
        "conversation_id": conversation_id,
        "reused": false,
        "cookies_applied": cookies_applied,
        "source": opts.source,
        "url": initial_url_for_event,
    }))
}

/// Start a new Browser Agent session for `conversation_id`. If a session is
/// already running for that conversation the existing one is returned and
/// no new Chromium is launched.
///
/// `profile_name` opts into a persistent named profile; `None` → disposable.
/// `initial_url` is the page loaded on launch; defaults to `about:blank`.
#[tauri::command]
pub async fn start_browser_session(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
    profile_name: Option<String>,
    initial_url: Option<String>,
    chrome_executable: Option<String>,
    cookie_session_id: Option<String>,
) -> Result<serde_json::Value, String> {
    start_browser_session_core(
        &app,
        state.inner(),
        conversation_id,
        StartSessionOptions {
            profile_name,
            initial_url,
            chrome_executable,
            cookie_session_id,
            source: "user".to_string(),
        },
    )
    .await
}

/// Tear down the session and its Chromium process.
#[tauri::command]
pub async fn stop_browser_session(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<(), String> {
    if let Some(session) = state.browser_sessions.remove(&conversation_id).await {
        session.controller.shutdown().await;
        session.node_lookup.forget_session(&session.id);
    }
    Ok(())
}

/// Start streaming the viewport to the frontend via `browser_agent_frame`
/// Tauri events. Safe to call repeatedly — the controller stops any previous
/// screencast before starting a new one.
#[tauri::command]
pub async fn start_browser_screencast(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
    quality: Option<i32>,
    max_width: Option<u32>,
    max_height: Option<u32>,
    every_nth_frame: Option<i32>,
) -> Result<(), String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session for this conversation".to_string())?;

    let sid = session.id.clone();
    let app_clone = app.clone();
    session
        .controller
        .start_screencast(
            quality.unwrap_or(60),
            max_width.unwrap_or(1280),
            max_height.unwrap_or(800),
            every_nth_frame.unwrap_or(2),
            move |data, w, h| {
                let frame = BrowserAgentFrame {
                    session_id: sid.clone(),
                    data_base64: data,
                    width: w,
                    height: h,
                    ts_ms: now_ms(),
                };
                events::emit_frame(&app_clone, &frame);
            },
        )
        .await
        .map_err(|e| format!("failed to start screencast: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn stop_browser_screencast(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<(), String> {
    if let Some(session) = state.browser_sessions.get(&conversation_id).await {
        session.controller.stop_screencast().await;
    }
    Ok(())
}

/// Pause the session: further CDP input dispatches are rejected until
/// `resume_browser_session` is called. Triggered by the global Ctrl+Shift+X
/// shortcut from the frontend.
#[tauri::command]
pub async fn pause_browser_session(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<bool, String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;
    session.controller.set_paused(true);
    Ok(true)
}

#[tauri::command]
pub async fn resume_browser_session(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<bool, String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;
    session.controller.set_paused(false);
    Ok(false)
}

/// Toggle "user take-over" mode. When true, the agent still holds the Page
/// but the frontend canvas forwards user input directly. The agent's own
/// tools continue to function.
#[tauri::command]
pub async fn set_browser_takeover(
    state: State<'_, AppState>,
    conversation_id: String,
    takeover: bool,
) -> Result<bool, String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;
    session.controller.set_takeover(takeover);
    Ok(takeover)
}

/// Forward a user-originated mouse event from the canvas into Chromium.
///
/// `kind` is `"move" | "down" | "up" | "wheel"`.
#[tauri::command]
pub async fn forward_browser_mouse(
    state: State<'_, AppState>,
    conversation_id: String,
    kind: String,
    x: f64,
    y: f64,
    button: Option<String>,
    click_count: Option<i64>,
) -> Result<(), String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;

    // Only allow forwarded input when the user has explicitly taken over. The
    // alternative is to accept input while paused or always — both make it
    // too easy for the frontend to race the agent.
    if !session.controller.is_takeover() {
        return Ok(());
    }

    use chromiumoxide::cdp::browser_protocol::input::{
        DispatchMouseEventType, MouseButton,
    };
    let kind = match kind.as_str() {
        "down" => DispatchMouseEventType::MousePressed,
        "up" => DispatchMouseEventType::MouseReleased,
        "wheel" => DispatchMouseEventType::MouseWheel,
        _ => DispatchMouseEventType::MouseMoved,
    };
    let btn = match button.as_deref() {
        Some("left") => Some(MouseButton::Left),
        Some("right") => Some(MouseButton::Right),
        Some("middle") => Some(MouseButton::Middle),
        _ => None,
    };
    session
        .controller
        .dispatch_mouse(kind, x, y, btn, click_count)
        .await
        .map_err(|e| format!("dispatch_mouse failed: {}", e))?;
    Ok(())
}

/// Forward a user-originated keyboard event. `kind` is `"down" | "up" | "char"`.
#[tauri::command]
pub async fn forward_browser_key(
    state: State<'_, AppState>,
    conversation_id: String,
    kind: String,
    key: Option<String>,
    code: Option<String>,
    text: Option<String>,
) -> Result<(), String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;
    if !session.controller.is_takeover() {
        return Ok(());
    }

    use chromiumoxide::cdp::browser_protocol::input::DispatchKeyEventType;
    let t = match kind.as_str() {
        "up" => DispatchKeyEventType::KeyUp,
        "char" => DispatchKeyEventType::Char,
        _ => DispatchKeyEventType::KeyDown,
    };
    session
        .controller
        .dispatch_key(t, key, text, code)
        .await
        .map_err(|e| format!("dispatch_key failed: {}", e))?;
    Ok(())
}

/// Drive the browser directly from the toolbar (URL bar, back/reload button).
/// Bypasses the LLM loop so the user always stays in control of the chrome.
#[tauri::command]
pub async fn browser_toolbar_navigate(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
    url: String,
) -> Result<(), String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;

    // Safety-gate the manual navigation too — blocklists must still apply.
    if state.browser_safety.is_blocked_domain(&url) {
        return Err(format!("Domain is blocked by safety policy: {}", url));
    }

    session
        .controller
        .navigate(&url)
        .await
        .map_err(|e| format!("navigate failed: {}", e))?;

    // Auto-apply any vault sessions that match the new host. Uses the
    // landed URL where possible (handles redirects, e.g. google.com →
    // www.google.com) and falls back to the requested URL otherwise.
    let landed = session
        .controller
        .current_url()
        .await
        .unwrap_or_else(|_| url.clone());
    match session
        .auto_inject_cookies(&landed, &state.browser_cookie_vault)
        .await
    {
        Ok(n) if n > 0 => log::info!(
            "[browser-agent] auto-injected {} cookie(s) for {}",
            n,
            landed
        ),
        Ok(_) => {}
        Err(e) => log::warn!(
            "[browser-agent] auto cookie inject failed for {}: {}",
            landed,
            e
        ),
    }

    events::emit_url(&app, &session.id, &landed);
    if let Ok(title) = session.controller.current_title().await {
        events::emit_title(&app, &session.id, &title);
    }
    Ok(())
}

#[tauri::command]
pub async fn browser_toolbar_reload(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<(), String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;
    session
        .controller
        .reload()
        .await
        .map_err(|e| format!("reload failed: {}", e))?;
    if let Ok(current) = session.controller.current_url().await {
        events::emit_url(&app, &session.id, &current);
    }
    Ok(())
}

#[tauri::command]
pub async fn browser_toolbar_back(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<(), String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;
    session
        .controller
        .go_back()
        .await
        .map_err(|e| format!("go_back failed: {}", e))?;
    if let Ok(current) = session.controller.current_url().await {
        events::emit_url(&app, &session.id, &current);
    }
    Ok(())
}

#[tauri::command]
pub async fn browser_toolbar_forward(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<(), String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session".to_string())?;
    session
        .controller
        .go_forward()
        .await
        .map_err(|e| format!("go_forward failed: {}", e))?;
    if let Ok(current) = session.controller.current_url().await {
        events::emit_url(&app, &session.id, &current);
    }
    Ok(())
}

/// Status snapshot for the toolbar.
#[tauri::command]
pub async fn get_browser_session_status(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<Option<serde_json::Value>, String> {
    let Some(session) = state.browser_sessions.get(&conversation_id).await else {
        return Ok(None);
    };
    let url = session.controller.current_url().await.unwrap_or_default();
    let title = session.controller.current_title().await.unwrap_or_default();
    Ok(Some(serde_json::json!({
        "session_id": session.id,
        "url": url,
        "title": title,
        "paused": session.controller.is_paused(),
        "takeover": session.controller.is_takeover(),
    })))
}

// ───────────────────────── Cookie sessions ──────────────────────────
//
// User-facing CRUD for the disk-backed CookieVault. Surfaced to the
// frontend so the user can paste cookies once in Settings → Browser and
// reuse them across browser-agent sessions without logging in every time.

/// List saved cookie sessions (metadata only — never returns raw values).
#[tauri::command]
pub async fn list_browser_cookie_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<CookieSessionDigest>, String> {
    Ok(state.browser_cookie_vault.list_digests())
}

/// Fetch the parse preview for a paste — used by the "Add session" form to
/// show the user how many cookies were detected before they hit save. The
/// values are NOT returned, only count + domains.
#[tauri::command]
pub async fn preview_cookie_paste(
    raw: String,
    default_domain: Option<String>,
) -> Result<serde_json::Value, String> {
    let cookies = cookie_vault::parse_cookies(&raw, default_domain.as_deref())
        .map_err(|e| format!("could not parse cookies: {}", e))?;
    let mut domains: Vec<String> = cookies
        .iter()
        .filter_map(|c| c.domain.clone())
        .map(|d| d.trim_start_matches('.').to_string())
        .collect();
    domains.sort();
    domains.dedup();
    Ok(serde_json::json!({
        "cookie_count": cookies.len(),
        "domains": domains,
        "missing_domain": cookies.iter().filter(|c| c.domain.is_none()).count(),
    }))
}

#[tauri::command]
pub async fn save_browser_cookie_session(
    state: State<'_, AppState>,
    name: String,
    raw: String,
    default_domain: Option<String>,
    notes: Option<String>,
) -> Result<BrowserCookieSession, String> {
    let dom = default_domain
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let cookies: Vec<CookieEntry> = cookie_vault::parse_cookies(&raw, dom.as_deref())
        .map_err(|e| format!("could not parse cookies: {}", e))?;
    state
        .browser_cookie_vault
        .create(name, cookies, notes.unwrap_or_default(), dom)
        .map_err(|e| format!("could not save: {}", e))
}

#[tauri::command]
pub async fn update_browser_cookie_session(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    raw: Option<String>,
    default_domain: Option<Option<String>>,
    notes: Option<String>,
) -> Result<BrowserCookieSession, String> {
    // Re-parse cookies only if the user pasted new ones. Use the explicit
    // `default_domain` from this update if provided, otherwise inherit the
    // existing one from the stored session.
    let cookies = match raw.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            let dom_for_parse: Option<String> = default_domain
                .as_ref()
                .and_then(|d| d.clone())
                .or_else(|| {
                    state
                        .browser_cookie_vault
                        .get(&id)
                        .and_then(|sess| sess.default_domain)
                });
            Some(
                cookie_vault::parse_cookies(s, dom_for_parse.as_deref())
                    .map_err(|e| format!("could not parse cookies: {}", e))?,
            )
        }
        _ => None,
    };
    state
        .browser_cookie_vault
        .update(&id, name, cookies, notes, default_domain)
        .map_err(|e| format!("could not update: {}", e))
}

#[tauri::command]
pub async fn delete_browser_cookie_session(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state
        .browser_cookie_vault
        .delete(&id)
        .map_err(|e| format!("could not delete: {}", e))
}

/// Manually inject cookies for the currently loaded page (or any URL you
/// supply). Searches the vault by domain and applies every matching session
/// that hasn't been applied yet.
///
/// Called by the frontend "Inject cookies" button so the user can re-inject
/// after navigating somewhere that loaded before the auto-inject path ran,
/// or to force a re-check after adding new vault sessions mid-session.
///
/// Returns a JSON object:
/// ```json
/// { "cookies_applied": 5, "sessions_applied": ["My Google Login"] }
/// ```
#[tauri::command]
pub async fn inject_cookies(
    state: State<'_, AppState>,
    conversation_id: String,
    // URL to match against. If omitted or empty the current page URL is used.
    url: Option<String>,
) -> Result<serde_json::Value, String> {
    let session = state
        .browser_sessions
        .get(&conversation_id)
        .await
        .ok_or_else(|| "No active browser session for this conversation".to_string())?;

    // Resolve the URL: explicit param → landed URL → error.
    let target_url = match url.as_deref() {
        Some(u) if !u.trim().is_empty() => u.to_string(),
        _ => session
            .controller
            .current_url()
            .await
            .map_err(|e| format!("could not read current URL: {}", e))?,
    };

    // Find out which sessions WOULD match before applying, so we can
    // return their names to the frontend for display.
    let matching = state
        .browser_cookie_vault
        .sessions_for_domain(&target_url);

    if matching.is_empty() {
        return Ok(serde_json::json!({
            "cookies_applied": 0,
            "sessions_applied": [],
            "url": target_url,
            "message": "No saved sessions match this domain.",
        }));
    }

    // Filter to sessions not yet applied and report.
    let unapplied: Vec<_> = {
        let guard = session
            .applied_cookie_sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        matching
            .iter()
            .filter(|s| !guard.contains(&s.id))
            .cloned()
            .collect()
    };

    if unapplied.is_empty() {
        let names: Vec<String> = matching.iter().map(|s| s.name.clone()).collect();
        return Ok(serde_json::json!({
            "cookies_applied": 0,
            "sessions_applied": names,
            "url": target_url,
            "message": "Matching sessions were already applied.",
        }));
    }

    let session_names: Vec<String> = unapplied.iter().map(|s| s.name.clone()).collect();
    let n = session
        .auto_inject_cookies(&target_url, &state.browser_cookie_vault)
        .await
        .map_err(|e| format!("inject failed: {}", e))?;

    log::info!(
        "[browser-agent] inject_cookies: applied {} cookie(s) from {:?} for {}",
        n,
        session_names,
        target_url
    );

    Ok(serde_json::json!({
        "cookies_applied": n,
        "sessions_applied": session_names,
        "url": target_url,
    }))
}

// ── LLM autostart preference ────────────────────────────────────────────────

const AUTOSTART_SETTING_KEY: &str = "browser_agent_autostart";

/// Read the "LLM can start the browser by itself" preference. Defaults to
/// `true` — the toggle is opt-out because the feature is gated behind an
/// explicit tool call the model has to choose to invoke.
#[tauri::command]
pub async fn get_browser_agent_autostart(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.browser_agent_autostart_allowed())
}

/// Persist the "LLM can start the browser by itself" preference. Writing
/// `false` makes the `browser_agent__start_session` tool return a structured
/// error instructing the LLM to defer to the user.
#[tauri::command]
pub async fn set_browser_agent_autostart(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let db = state
        .db
        .lock()
        .map_err(|e| format!("db mutex poisoned: {}", e))?;
    db.set_setting(AUTOSTART_SETTING_KEY, if enabled { "1" } else { "0" })
        .map_err(|e| format!("could not persist setting: {}", e))?;
    Ok(())
}
