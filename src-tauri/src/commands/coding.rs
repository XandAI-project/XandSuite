use chrono::Utc;
use rusqlite::params;
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::coding::runtime::{
    CodingEvent, CodingMessage, CodingPlan, CodingSession,
};
use crate::state::AppState;

// ── Session management ────────────────────────────────────────────────────────

/// Create a new coding session.
#[tauri::command]
pub fn create_coding_session(
    mode: String,
    project_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<CodingSession, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let title = format!("{} session", capitalize(&mode));

    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "INSERT INTO coding_sessions (id, title, mode, project_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, title, mode, project_path, now, now],
        )
        .map_err(|e| e.to_string())?;

    Ok(CodingSession {
        id,
        title,
        mode,
        project_path,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// List recent coding sessions (newest first).
#[tauri::command]
pub fn list_coding_sessions(state: State<'_, AppState>) -> Result<Vec<CodingSession>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, title, mode, project_path, created_at, updated_at
             FROM coding_sessions ORDER BY created_at DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;

    let sessions = stmt
        .query_map([], |row| {
            Ok(CodingSession {
                id: row.get(0)?,
                title: row.get(1)?,
                mode: row.get(2)?,
                project_path: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(sessions)
}

/// Get full session with messages.
#[tauri::command]
pub fn get_coding_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(CodingSession, Vec<CodingMessage>), String> {
    let db = state.db.lock().unwrap();

    let session: CodingSession = db
        .conn
        .query_row(
            "SELECT id, title, mode, project_path, created_at, updated_at
             FROM coding_sessions WHERE id = ?1",
            params![session_id],
            |row| {
                Ok(CodingSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    mode: row.get(2)?,
                    project_path: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, session_id, role, content, events_json, created_at
             FROM coding_messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let messages: Vec<CodingMessage> = stmt
        .query_map(params![session_id], |row| {
            let events_json: String = row.get(4)?;
            let events = serde_json::from_str(&events_json).unwrap_or_default();
            Ok(CodingMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                events,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok((session, messages))
}

/// Update session title or project path.
#[tauri::command]
pub fn update_coding_session(
    session_id: String,
    title: Option<String>,
    mode: Option<String>,
    project_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    let now = Utc::now().to_rfc3339();
    if let Some(t) = title {
        db.conn
            .execute(
                "UPDATE coding_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![t, now, session_id],
            )
            .map_err(|e| e.to_string())?;
    }
    if let Some(m) = mode {
        db.conn
            .execute(
                "UPDATE coding_sessions SET mode = ?1, updated_at = ?2 WHERE id = ?3",
                params![m, now, session_id],
            )
            .map_err(|e| e.to_string())?;
    }
    if let Some(p) = project_path {
        db.conn
            .execute(
                "UPDATE coding_sessions SET project_path = ?1, updated_at = ?2 WHERE id = ?3",
                params![p, now, session_id],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Delete a coding session and its messages.
#[tauri::command]
pub fn delete_coding_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute("DELETE FROM coding_sessions WHERE id = ?1", params![session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Send message / run agent ──────────────────────────────────────────────────

/// Send a coding message; triggers the ReAct loop in the background.
/// Returns the user message ID immediately; the AI response arrives via `coding_event`.
#[tauri::command]
pub async fn send_coding_message(
    app: AppHandle,
    session_id: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<CodingMessage, String> {
    // Load session info
    let (session, history) = {
        let db = state.db.lock().unwrap();
        let session: CodingSession = db
            .conn
            .query_row(
                "SELECT id, title, mode, project_path, created_at, updated_at
                 FROM coding_sessions WHERE id = ?1",
                params![session_id],
                |row| {
                    Ok(CodingSession {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        mode: row.get(2)?,
                        project_path: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .map_err(|e| e.to_string())?;

        // Load message history for context
        let mut stmt = db
            .conn
            .prepare(
                "SELECT role, content FROM coding_messages
                 WHERE session_id = ?1 ORDER BY created_at ASC LIMIT 40",
            )
            .map_err(|e| e.to_string())?;
        let history: Vec<(String, String)> = stmt
            .query_map(params![session_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        (session, history)
    };

    // Persist user message
    let user_msg = save_message(&state, &session_id, "user", &content, vec![])?;

    // Auto-title session from first message
    if history.is_empty() {
        let auto_title: String = content.chars().take(60).collect();
        let db = state.db.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let _ = db.conn.execute(
            "UPDATE coding_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![auto_title, now, session_id],
        );
    }

    // Register cancellation flag
    let cancel_flag = Arc::new(AtomicBool::new(false));
    state
        .coding_runtime
        .register_cancel(&session_id, cancel_flag.clone());

    // Event channel: runtime → frontend
    let (event_tx, mut event_rx) = mpsc::channel::<CodingEvent>(256);

    let app_clone = app.clone();
    let _sid_clone = session_id.clone();
    tokio::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            let _ = app_clone.emit(
                "coding_event",
                json!({
                    "session_id": ev.session_id,
                    "event_type": ev.event_type.as_str(),
                    "payload": ev.payload,
                }),
            );
        }
    });

    // Spawn the ReAct loop
    let runtime = state.coding_runtime.clone();
    let engine = state.engine.clone();
    let db = state.db.clone();
    let sid = session_id.clone();

    tokio::spawn(async move {
        match runtime
            .run(
                sid.clone(),
                content,
                session.mode.clone(),
                session.project_path.clone(),
                history,
                engine,
                event_tx.clone(),
            )
            .await
        {
            Ok(answer) => {
                // Persist assistant message
                let _ = save_message_arc(&db, &sid, "assistant", &answer, vec![]);
            }
            Err(e) => {
                log::error!("Coding runtime error for session {}: {}", sid, e);
                let _ = save_message_arc(&db, &sid, "assistant", &format!("Error: {}", e), vec![]);
            }
        }
    });

    Ok(user_msg)
}

/// Cancel a running coding session.
#[tauri::command]
pub fn cancel_coding_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.coding_runtime.cancel_session(&session_id);
    Ok(())
}

// ── Filesystem ────────────────────────────────────────────────────────────────

/// Open a folder picker dialog and return the selected path.
#[tauri::command]
pub async fn select_coding_project(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app
        .dialog()
        .file()
        .blocking_pick_folder();
    Ok(path.map(|p| p.to_string()))
}

/// List directory contents as a tree for the file explorer.
#[tauri::command]
pub async fn list_coding_directory(
    project_path: String,
    sub_path: Option<String>,
    depth: Option<u64>,
) -> Result<Value, String> {
    use crate::coding::tools::CodingToolExecutor;
    let executor = CodingToolExecutor::new(std::path::PathBuf::from(&project_path));
    let path = sub_path.unwrap_or_else(|| ".".to_string());
    let d = depth.unwrap_or(4);
    let input = json!({ "path": path, "depth": d });
    executor
        .execute("directory_tree", &input)
        .await
        .map_err(|e| e.to_string())
}

/// Read a file for the file preview panel.
#[tauri::command]
pub async fn read_coding_file(
    project_path: String,
    file_path: String,
) -> Result<String, String> {
    use crate::coding::tools::CodingToolExecutor;
    let executor = CodingToolExecutor::new(std::path::PathBuf::from(&project_path));
    let input = json!({ "path": file_path });
    let result = executor
        .execute("file_read", &input)
        .await
        .map_err(|e| e.to_string())?;
    Ok(result["content"].as_str().unwrap_or("").to_string())
}

// ── Plans ─────────────────────────────────────────────────────────────────────

/// Get the latest plan for a session.
#[tauri::command]
pub fn get_coding_plan(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Option<CodingPlan>, String> {
    let db = state.db.lock().unwrap();
    let result = db.conn.query_row(
        "SELECT id, session_id, tasks_json, status, created_at, updated_at
         FROM coding_plans WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 1",
        params![session_id],
        |row| {
            let tasks_json: String = row.get(2)?;
            let tasks = serde_json::from_str(&tasks_json).unwrap_or_default();
            Ok(CodingPlan {
                id: row.get(0)?,
                session_id: row.get(1)?,
                title: String::new(),
                tasks,
                status: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        },
    );

    match result {
        Ok(plan) => Ok(Some(plan)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn save_message(
    state: &State<'_, AppState>,
    session_id: &str,
    role: &str,
    content: &str,
    events: Vec<crate::coding::runtime::CodingEventPayload>,
) -> Result<CodingMessage, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let events_json = serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string());

    let db = state.db.lock().unwrap();
    db.conn
        .execute(
            "INSERT INTO coding_messages (id, session_id, role, content, events_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, session_id, role, content, events_json, now],
        )
        .map_err(|e| e.to_string())?;

    Ok(CodingMessage {
        id,
        session_id: session_id.to_string(),
        role: role.to_string(),
        content: content.to_string(),
        events,
        created_at: now,
    })
}

fn save_message_arc(
    db: &Arc<std::sync::Mutex<crate::db::AppDb>>,
    session_id: &str,
    role: &str,
    content: &str,
    events: Vec<crate::coding::runtime::CodingEventPayload>,
) -> Result<(), String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let events_json = serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string());
    let db = db.lock().unwrap();
    db.conn
        .execute(
            "INSERT INTO coding_messages (id, session_id, role, content, events_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, session_id, role, content, events_json, now],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
