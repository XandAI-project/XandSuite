use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::models::RetrievalMode;
use crate::state::AppState;

pub async fn list_rag_collections(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let rag = state.rag.lock().await;
    let collections = rag.list_collections()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(collections).unwrap_or(serde_json::json!([]))))
}

#[derive(Deserialize)]
pub struct CreateCollectionBody {
    pub name: String,
    pub description: Option<String>,
}

pub async fn create_rag_collection(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateCollectionBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let rag = state.rag.lock().await;
    let coll = rag
        .create_collection(&body.name, body.description.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "id": coll.id, "name": coll.name })))
}

pub async fn delete_rag_collection(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let rag = state.rag.lock().await;
    rag.delete_collection(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct IngestBody {
    pub text: Option<String>,
    pub source: Option<String>,
}

pub async fn ingest_document(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Json(body): Json<IngestBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let text = body.text.ok_or_else(|| (StatusCode::BAD_REQUEST, "text required".to_string()))?;
    let source = body.source.unwrap_or_else(|| "upload".to_string());
    let rag = state.rag.lock().await;
    rag.ingest_text(&collection_id, &text, &source, &state.embedder)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct SearchBody {
    pub query: String,
    pub collection_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct SetModeBody {
    pub mode: String,
}

pub async fn set_retrieval_mode(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Json(body): Json<SetModeBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let retrieval_mode = if body.mode == "graph" {
        RetrievalMode::Graph
    } else {
        RetrievalMode::Hybrid
    };
    let rag = state.rag.lock().await;
    rag.set_retrieval_mode(&collection_id, retrieval_mode, false)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn search_rag(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SearchBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let cosine_weight = state.settings.lock().unwrap().hybrid_cosine_weight;
    let rag = state.rag.lock().await;
    let results = rag.search(
        &body.query,
        body.collection_id.as_deref(),
        body.limit.unwrap_or(10),
        &state.embedder,
        cosine_weight,
    ).await;
    Ok(Json(serde_json::to_value(results).unwrap_or(serde_json::json!([]))))
}
