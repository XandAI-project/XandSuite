use tauri::{Emitter, State};

use crate::models::RetrievalMode;
use crate::state::AppState;

macro_rules! gr_log {
    ($state:expr, $level:expr, $fmt:literal $(, $arg:expr)*) => {{
        let msg = format!($fmt $(, $arg)*);
        log::info!("{}", msg);
        let _ = $state.app_handle.emit("app_log", serde_json::json!({
            "level": $level,
            "message": msg,
            "ts": chrono::Utc::now().to_rfc3339(),
        }));
    }};
}

/// Returns the current status of the graphrag-server sidecar.
#[tauri::command]
pub async fn graph_rag_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let mut mgr = state.graph_rag.lock().await;
    let running = mgr.is_running();
    let enabled = state.settings.lock().unwrap().graph_rag_enabled;
    let port = mgr.port();

    // Also do a live health-check when the process appears running
    let reachable = if running {
        if let Some(client) = state.graph_rag_client.as_ref() {
            client.health().await
        } else {
            false
        }
    } else {
        false
    };

    gr_log!(
        state, "info",
        "[graphrag] Status — enabled={} process_running={} reachable={} port={}",
        enabled, running, reachable, port
    );

    Ok(serde_json::json!({
        "running": running,
        "enabled": enabled,
        "reachable": reachable,
        "port": port,
    }))
}

/// Manually start the graphrag-server sidecar (if enabled).
#[tauri::command]
pub async fn start_graph_rag(state: State<'_, AppState>) -> Result<(), String> {
    let (enabled, port, vector_db, server_path, embedding_model) = {
        let s = state.settings.lock().unwrap();
        (
            s.graph_rag_enabled,
            s.graph_rag_port,
            s.graph_rag_vector_db.clone(),
            s.graph_rag_server_path.clone(),
            s.embedding_model.clone(),
        )
    };

    if !enabled {
        let msg = "[graphrag] Cannot start — GraphRAG is not enabled in settings.";
        gr_log!(state, "error", "{}", msg);
        return Err(msg.to_string());
    }

    gr_log!(state, "info", "[graphrag] Starting server on port {} (vector_db={}, model={})", port, vector_db, embedding_model);

    let mut mgr = state.graph_rag.lock().await;
    mgr.start(&state.data_dir, port, &vector_db, &embedding_model, server_path.as_deref())
        .map_err(|e| {
            let msg = format!("[graphrag] Failed to start: {}", e);
            gr_log!(state, "error", "{}", msg);
            msg
        })?;

    gr_log!(state, "info", "[graphrag] Process spawned — waiting up to 30 s for /health…");
    mgr.wait_ready(30).await.map_err(|e| {
        let msg = format!("[graphrag] Server did not become ready: {}", e);
        gr_log!(state, "error", "{}", msg);
        msg
    })?;

    gr_log!(state, "info", "[graphrag] Server is ready and healthy.");
    Ok(())
}

/// Manually stop the graphrag-server sidecar.
#[tauri::command]
pub async fn stop_graph_rag(state: State<'_, AppState>) -> Result<(), String> {
    gr_log!(state, "info", "[graphrag] Stopping server…");
    let mut mgr = state.graph_rag.lock().await;
    mgr.stop();
    gr_log!(state, "info", "[graphrag] Server stopped.");
    Ok(())
}

/// Ingest a single document into the graph index for a specific collection.
#[tauri::command]
pub async fn ingest_to_graph(
    collection_id: String,
    title: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    gr_log!(state, "info", "[graphrag] Ingesting '{}' into collection id={}", title, collection_id);

    let client = state.graph_rag_client.as_ref().ok_or_else(|| {
        let msg = "[graphrag] Client not initialised — is the sidecar running?";
        gr_log!(state, "error", "{}", msg);
        msg.to_string()
    })?;

    client.ingest(&collection_id, &title, &content).await.map_err(|e| {
        let msg = format!("[graphrag] Ingest failed for '{}': {}", title, e);
        gr_log!(state, "error", "{}", msg);
        msg
    })?;

    gr_log!(state, "info", "[graphrag] Ingest ok for '{}' in collection id={}", title, collection_id);

    // Mark the collection as graph-indexed after a successful ingest
    let rag = state.rag.lock().await;
    if let Err(e) = rag.set_retrieval_mode(&collection_id, RetrievalMode::Graph, true) {
        gr_log!(state, "warn", "[graphrag] Could not mark graph_indexed=true: {}", e);
    }

    Ok(())
}

/// Query the graph index for a specific collection.
#[tauri::command]
pub async fn query_graph(
    collection_id: String,
    query: String,
    top_k: Option<usize>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let k = top_k.unwrap_or(8);
    gr_log!(state, "info", "[graphrag] Query collection={} query='{}' top_k={}", collection_id, query, k);

    let client = state.graph_rag_client.as_ref().ok_or_else(|| {
        let msg = "[graphrag] Client not initialised — is the sidecar running?";
        gr_log!(state, "error", "{}", msg);
        msg.to_string()
    })?;

    let results = client.query(&collection_id, &query, k).await.map_err(|e| {
        let msg = format!("[graphrag] Query failed: {}", e);
        gr_log!(state, "error", "{}", msg);
        msg
    })?;

    gr_log!(state, "info", "[graphrag] Query returned {} result(s)", results.len());
    serde_json::to_value(results).map_err(|e| e.to_string())
}
