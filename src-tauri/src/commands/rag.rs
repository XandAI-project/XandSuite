use std::path::PathBuf;
use tauri::State;

use crate::models::{Document, RagCollection};
use crate::state::AppState;

#[tauri::command]
pub async fn list_rag_collections(state: State<'_, AppState>) -> Result<Vec<RagCollection>, String> {
    let rag = state.rag.lock().await;
    rag.list_collections().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_rag_collection(
    name: String,
    description: Option<String>,
    state: State<'_, AppState>,
) -> Result<RagCollection, String> {
    let rag = state.rag.lock().await;
    rag.create_collection(&name, description.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_rag_collection(
    collection_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let rag = state.rag.lock().await;
    rag.delete_collection(&collection_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ingest_document(
    collection_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<Vec<Document>, String> {
    let path = PathBuf::from(&file_path);
    let rag = state.rag.lock().await;
    rag.ingest_file(&collection_id, &path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_rag(
    query: String,
    collection_id: Option<String>,
    top_k: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let rag = state.rag.lock().await;
    let results = rag.search(&query, collection_id.as_deref(), top_k.unwrap_or(5));
    let json_results = results
        .into_iter()
        .map(|r| serde_json::json!({
            "content": r.chunk.content,
            "score": r.score,
            "source": r.chunk.metadata.get("source").and_then(|v| v.as_str()).unwrap_or(""),
            "metadata": r.chunk.metadata
        }))
        .collect();
    Ok(json_results)
}
