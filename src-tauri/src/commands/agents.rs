use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use uuid::Uuid;

fn task_workspace_dir(task_id: &str) -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.xandnet.xandsuite")
        .join("agent_workspace")
        .join(task_id)
}

use crate::agent::runtime::{AgentEvent, AgentEventType};
use crate::models::{AgentTask, AgentTaskStatus};
use crate::state::AppState;

/// Create an agent task record, launch the ReAct loop in the background, and
/// return the initial task immediately (status = Running).
/// The UI tracks progress via `agent_event` Tauri events.
#[tauri::command]
pub async fn run_agent_task(
    app: AppHandle,
    task_description: String,
    state: State<'_, AppState>,
) -> Result<AgentTask, String> {
    let task_id = Uuid::new_v4().to_string();
    let now = Utc::now();

    let task = AgentTask {
        id: task_id.clone(),
        title: task_description.chars().take(80).collect(),
        description: task_description.clone(),
        status: AgentTaskStatus::Running,
        steps: vec![],
        created_at: now,
        completed_at: None,
        result: None,
    };

    // Persist the initial record synchronously before spawning
    {
        let db = state.db.lock().unwrap();
        db.conn
            .execute(
                r#"INSERT INTO agent_tasks
                   (id, title, description, status, steps_json, created_at, completed_at, result)
                   VALUES (?1, ?2, ?3, 'running', '[]', ?4, NULL, NULL)"#,
                params![task.id, task.title, task.description, now.to_rfc3339()],
            )
            .map_err(|e| e.to_string())?;
    }

    // Register a cancellation flag before spawning so cancel_task can reach it
    let cancel_flag = Arc::new(AtomicBool::new(false));
    state
        .agent_runtime
        .register_cancel(&task_id, cancel_flag.clone());

    // Channel: runtime → frontend event emitter
    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(128);

    let app_clone = app.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let event_type = match event.event_type {
                AgentEventType::Started => "started",
                AgentEventType::LlmGenerating => "llm_generating",
                AgentEventType::Thought => "thought",
                AgentEventType::Action => "action",
                AgentEventType::Observation => "observation",
                AgentEventType::Completed => "completed",
                AgentEventType::Failed => "failed",
                AgentEventType::Cancelled => "cancelled",
            };
            let _ = app_clone.emit(
                "agent_event",
                json!({
                    "task_id": event.task_id,
                    "event_type": event_type,
                    "payload": event.payload,
                }),
            );
        }
    });

    // Emit the Started event immediately so the UI switches to live mode
    let _ = event_tx
        .send(AgentEvent {
            task_id: task_id.clone(),
            event_type: AgentEventType::Started,
            payload: json!({ "task": task_description }),
        })
        .await;

    // Spawn the ReAct loop — fire and forget; the command returns right away
    let runtime = state.agent_runtime.clone();
    let engine = state.engine.clone();
    let task_clone = task.clone();
    tokio::spawn(async move {
        if let Err(e) = runtime.run_loop(task_clone, engine, event_tx).await {
            log::error!("Agent runtime error for task {}: {}", task_id, e);
        }
    });

    Ok(task)
}

/// Public inner function so the HTTP handler can call the same logic without AppState wrapper.
pub async fn run_agent_task_inner(
    app: AppHandle,
    title: String,
    description: String,
    state: &AppState,
) -> Result<String, String> {
    let task_id = Uuid::new_v4().to_string();
    let now = Utc::now();

    {
        let db = state.db.lock().unwrap();
        db.conn
            .execute(
                r#"INSERT INTO agent_tasks
                   (id, title, description, status, steps_json, created_at, completed_at, result)
                   VALUES (?1, ?2, ?3, 'running', '[]', ?4, NULL, NULL)"#,
                params![task_id, title, description, now.to_rfc3339()],
            )
            .map_err(|e| e.to_string())?;
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    state.agent_runtime.register_cancel(&task_id, cancel_flag.clone());

    let (event_tx_inner, mut event_rx) = mpsc::channel::<AgentEvent>(128);
    let app_clone = app.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let event_type = match event.event_type {
                AgentEventType::Started => "started",
                AgentEventType::LlmGenerating => "llm_generating",
                AgentEventType::Thought => "thought",
                AgentEventType::Action => "action",
                AgentEventType::Observation => "observation",
                AgentEventType::Completed => "completed",
                AgentEventType::Failed => "failed",
                AgentEventType::Cancelled => "cancelled",
            };
            let _ = app_clone.emit("agent_event", json!({
                "task_id": event.task_id,
                "event_type": event_type,
                "payload": event.payload,
            }));
            // Also broadcast to HTTP SSE clients
            use tauri::Manager;
            if let Some(st) = app_clone.try_state::<crate::state::AppState>() {
                let _ = st.event_tx.send(crate::api_server::events::ApiEvent::AgentEvent {
                    task_id: event.task_id.clone(),
                    event_type: event_type.to_string(),
                    payload: event.payload.clone(),
                });
            }
        }
    });

    let _ = event_tx_inner.send(AgentEvent {
        task_id: task_id.clone(),
        event_type: AgentEventType::Started,
        payload: json!({ "task": description }),
    }).await;

    let runtime = state.agent_runtime.clone();
    let engine = state.engine.clone();
    let tid = task_id.clone();
    tokio::spawn(async move {
        if let Err(e) = runtime.run_loop(
            crate::models::AgentTask {
                id: tid.clone(),
                title: title.chars().take(80).collect(),
                description,
                status: crate::models::AgentTaskStatus::Running,
                steps: vec![],
                created_at: now,
                completed_at: None,
                result: None,
            },
            engine,
            event_tx_inner,
        ).await {
            log::error!("Agent runtime error for task {}: {}", tid, e);
        }
    });

    Ok(task_id)
}

/// List recent agent tasks (newest first).
#[tauri::command]
pub fn list_agent_tasks(state: State<'_, AppState>) -> Result<Vec<AgentTask>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, title, description, status, steps_json, created_at, completed_at, result
             FROM agent_tasks ORDER BY created_at DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;

    let tasks = stmt
        .query_map([], |row| {
            let steps_json: String = row.get(4)?;
            let steps = serde_json::from_str(&steps_json).unwrap_or_default();
            let status_str: String = row.get(3)?;
            let status = match status_str.as_str() {
                "running" => AgentTaskStatus::Running,
                "completed" => AgentTaskStatus::Completed,
                "failed" => AgentTaskStatus::Failed,
                "cancelled" => AgentTaskStatus::Cancelled,
                _ => AgentTaskStatus::Pending,
            };
            Ok(AgentTask {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                status,
                steps,
                created_at: Utc::now(),
                completed_at: None,
                result: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(tasks)
}

/// Delete an agent task by ID.
#[tauri::command]
pub fn delete_agent_task(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn
        .execute("DELETE FROM agent_tasks WHERE id = ?1", params![task_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Cancel a running agent task.
#[tauri::command]
pub fn cancel_agent_task(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Signal the runtime loop to stop at the next iteration boundary
    state.agent_runtime.cancel_task(&task_id);
    Ok(())
}

/// List files created in the agent task's workspace directory.
#[tauri::command]
pub async fn list_task_files(task_id: String) -> Result<Vec<serde_json::Value>, String> {
    let workspace = task_workspace_dir(&task_id);
    if !workspace.exists() {
        return Ok(vec![]);
    }
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&workspace)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(entry) = read_dir.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata().await.ok();
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        if !is_dir {
            entries.push(json!({ "name": name, "size_bytes": size }));
        }
    }
    // Sort by name for stable ordering
    entries.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });
    Ok(entries)
}

/// Read a file from the agent task workspace (returns base64-encoded content for binary safety).
#[tauri::command]
pub async fn read_task_file(task_id: String, filename: String) -> Result<String, String> {
    let path = task_workspace_dir(&task_id).join(&filename);
    // Guard against path traversal
    let workspace = task_workspace_dir(&task_id);
    let canonical = path.canonicalize().unwrap_or(path.clone());
    if !canonical.starts_with(&workspace) {
        return Err("Path traversal not allowed".to_string());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())
}

/// Open the task workspace directory in the OS file manager.
#[tauri::command]
pub async fn open_task_workspace(_app: AppHandle, task_id: String) -> Result<(), String> {
    let workspace = task_workspace_dir(&task_id);
    if !workspace.exists() {
        tokio::fs::create_dir_all(&workspace)
            .await
            .map_err(|e| e.to_string())?;
    }
    let path_str = workspace.to_string_lossy().to_string();
    tauri_plugin_opener::open_path(path_str, None::<String>)
        .map_err(|e| e.to_string())
        .map(|_| ())
}
