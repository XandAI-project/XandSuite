use std::path::PathBuf;
use tauri::{Emitter, State};

use crate::models::{Document, RagCollection, RetrievalMode};
use crate::state::AppState;


// ── Logging helper ─────────────────────────────────────────────────────────────

macro_rules! rag_log {
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

// ── Commands ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_rag_collections(state: State<'_, AppState>) -> Result<Vec<RagCollection>, String> {
    let rag = state.rag.lock().await;
    let collections = rag.list_collections().map_err(|e| e.to_string())?;
    rag_log!(state, "info", "[rag] Listed {} knowledge base(s)", collections.len());
    Ok(collections)
}

#[tauri::command]
pub async fn create_rag_collection(
    name: String,
    description: Option<String>,
    state: State<'_, AppState>,
) -> Result<RagCollection, String> {
    rag_log!(state, "info", "[rag] Creating knowledge base '{}'", name);
    let rag = state.rag.lock().await;
    let coll = rag.create_collection(&name, description.as_deref()).map_err(|e| {
        let msg = format!("[rag] Failed to create '{}': {}", name, e);
        rag_log!(state, "error", "{}", msg);
        msg
    })?;
    rag_log!(state, "info", "[rag] Knowledge base '{}' created (id={})", coll.name, coll.id);
    Ok(coll)
}

#[tauri::command]
pub async fn delete_rag_collection(
    collection_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    rag_log!(state, "info", "[rag] Deleting knowledge base id={}", collection_id);
    let rag = state.rag.lock().await;
    rag.delete_collection(&collection_id).map_err(|e| {
        let msg = format!("[rag] Delete failed for id={}: {}", collection_id, e);
        rag_log!(state, "error", "{}", msg);
        msg
    })?;
    rag_log!(state, "info", "[rag] Knowledge base id={} deleted", collection_id);
    Ok(())
}

#[tauri::command]
pub async fn ingest_document(
    collection_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<Vec<Document>, String> {
    let path = PathBuf::from(&file_path);
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or(&file_path).to_string();
    rag_log!(state, "info", "[rag] Ingesting '{}' into collection id={}", file_name, collection_id);

    let rag = state.rag.lock().await;
    let docs = rag.ingest_file(&collection_id, &path, &state.embedder).await.map_err(|e| {
        let msg = format!("[rag] Ingest failed for '{}': {}", file_name, e);
        rag_log!(state, "error", "{}", msg);
        msg
    })?;

    rag_log!(
        state, "info",
        "[rag] Ingested '{}' — produced {} chunk(s)", file_name, docs.len()
    );

    // If this collection is in Graph mode and the GraphRAG server is up,
    // automatically forward the newly ingested content.
    let mode_is_graph = {
        rag.list_collections()
            .ok()
            .and_then(|cols| cols.into_iter().find(|c| c.id == collection_id))
            .map(|c| c.retrieval_mode == RetrievalMode::Graph)
            .unwrap_or(false)
    };

    if mode_is_graph {
        if let Some(client) = state.graph_rag_client.as_ref() {
            if client.health().await {
                rag_log!(state, "info", "[rag] Collection is in Graph mode — forwarding '{}' to GraphRAG", file_name);
                for doc in &docs {
                    match client.ingest(&collection_id, &file_name, &doc.content).await {
                        Ok(_) => rag_log!(state, "info", "[rag] GraphRAG ingest ok for doc id={}", doc.id),
                        Err(e) => rag_log!(state, "warn", "[rag] GraphRAG ingest failed for doc id={}: {}", doc.id, e),
                    }
                }
            } else {
                rag_log!(state, "warn", "[rag] Graph mode active but GraphRAG server is not reachable — only hybrid index updated");
            }
        } else {
            rag_log!(state, "warn", "[rag] Graph mode active but GraphRAG client not initialised (enable in Settings → Knowledge Base)");
        }
    }

    Ok(docs)
}

#[tauri::command]
pub async fn search_rag(
    query: String,
    collection_id: Option<String>,
    top_k: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let k = top_k.unwrap_or(5);
    rag_log!(
        state, "info",
        "[rag] Search query='{}' collection={:?} top_k={}",
        query, collection_id, k
    );

    let cosine_weight = state.settings.lock().unwrap().hybrid_cosine_weight;
    let rag = state.rag.lock().await;
    let results = rag.search(
        &query,
        collection_id.as_deref(),
        k,
        &state.embedder,
        cosine_weight,
    ).await;

    rag_log!(
        state, "info",
        "[rag] Search returned {} result(s) (cosine_weight={:.2})",
        results.len(), cosine_weight
    );

    let json_results = results
        .into_iter()
        .map(|r| serde_json::json!({
            "content": r.chunk.content,
            "score": r.score,
            "source": r.chunk.metadata.get("source").and_then(|v| v.as_str()).unwrap_or(""),
            "metadata": r.chunk.metadata,
            "entities": r.entities,
            "relationships": r.relationships,
        }))
        .collect();
    Ok(json_results)
}

#[tauri::command]
pub async fn set_collection_retrieval_mode(
    collection_id: String,
    mode: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let retrieval_mode = if mode == "graph" {
        RetrievalMode::Graph
    } else {
        RetrievalMode::Hybrid
    };

    rag_log!(state, "info", "[rag] Setting retrieval mode='{}' for collection id={}", mode, collection_id);

    let rag = state.rag.lock().await;
    rag.set_retrieval_mode(&collection_id, retrieval_mode, false).map_err(|e| {
        let msg = format!("[rag] Failed to set mode: {}", e);
        rag_log!(state, "error", "{}", msg);
        msg
    })?;

    rag_log!(state, "info", "[rag] Retrieval mode set to '{}' for id={}", mode, collection_id);
    Ok(())
}

/// Re-index all documents in a collection into the GraphRAG server.
/// Marks `graph_indexed = true` when all documents have been ingested.
/// This is the operation that drives the "Indexing…" → "Indexed" transition.
#[tauri::command]
pub async fn reindex_collection(
    collection_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    rag_log!(state, "info", "[rag] Starting GraphRAG re-index for collection id={}", collection_id);

    let client = state.graph_rag_client.as_ref().ok_or_else(|| {
        let msg = "[rag] GraphRAG client not initialised — enable GraphRAG in Settings → Knowledge Base";
        rag_log!(state, "error", "{}", msg);
        msg.to_string()
    })?;

    // Health check before we start
    if !client.health().await {
        let msg = "[rag] GraphRAG server is not reachable. Start it via Settings → Knowledge Base → Start GraphRAG.";
        rag_log!(state, "error", "{}", msg);
        return Err(msg.to_string());
    }

    rag_log!(state, "info", "[rag] GraphRAG server is healthy — fetching documents from DB");

    // Fetch all (source, content) pairs for this collection
    let docs: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.conn.prepare(
            "SELECT source_file, content FROM rag_documents WHERE collection_id = ?1"
        ).map_err(|e| e.to_string())?;
        let rows: Vec<_> = stmt.query_map(rusqlite::params![collection_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
        rows
    };

    rag_log!(state, "info", "[rag] Found {} document(s) to index for collection id={}", docs.len(), collection_id);

    let total = docs.len();
    let mut ok = 0usize;
    let mut failed = 0usize;

    for (source, content) in &docs {
        match client.ingest(&collection_id, source, content).await {
            Ok(_) => {
                ok += 1;
                rag_log!(state, "info", "[rag] GraphRAG indexed [{}/{}] '{}'", ok, total, source);
            }
            Err(e) => {
                failed += 1;
                rag_log!(state, "warn", "[rag] GraphRAG index failed for '{}': {}", source, e);
            }
        }
    }

    rag_log!(
        state, "info",
        "[rag] Re-index complete — {}/{} documents indexed, {} failed",
        ok, total, failed
    );

    if failed > 0 && ok == 0 {
        return Err(format!("All {} document(s) failed to index. Check that the GraphRAG server is running.", total));
    }

    // Mark the collection as indexed
    let rag = state.rag.lock().await;
    rag.set_retrieval_mode(&collection_id, RetrievalMode::Graph, true)
        .map_err(|e| e.to_string())?;

    rag_log!(state, "info", "[rag] Collection id={} marked as graph_indexed=true", collection_id);
    Ok(())
}
