pub mod auth;
pub mod events;
pub mod handlers;

use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::state::AppState;

/// Build and start the HTTP/SSE bridge server.
/// Blocks forever; intended to be spawned in a background tokio task.
pub async fn start_api_server(state: Arc<AppState>, port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = build_router(state.clone())
        .layer(cors);

    let addr = format!("0.0.0.0:{}", port);
    log::info!("Mobile API server listening on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("Failed to bind mobile API server on {}: {}", addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        log::error!("Mobile API server error: {}", e);
    }
}

fn build_router(state: Arc<AppState>) -> Router {
    use handlers::*;

    let auth_layer = middleware::from_fn_with_state(state.clone(), auth::auth_middleware);

    let api = Router::new()
        // ── Events (SSE) ──────────────────────────────────────────────────
        .route("/events", get(events::sse_events))
        // ── Chat ─────────────────────────────────────────────────────────
        .route("/conversations", get(chat::list_conversations).post(chat::create_conversation))
        .route("/conversations/:id", get(chat::get_conversation).put(chat::update_conversation).delete(chat::delete_conversation))
        .route("/conversations/:id/truncate", post(chat::truncate_conversation))
        .route("/messages/send", post(chat::send_message))
        .route("/messages/:id/tool-steps", post(chat::save_message_tool_steps))
        // ── Artifacts ────────────────────────────────────────────────────
        .route("/artifacts", get(artifacts::list_artifacts).post(artifacts::save_artifact))
        .route("/artifacts/all", get(artifacts::list_all_artifacts))
        .route("/artifacts/:id", put(artifacts::update_artifact).delete(artifacts::delete_artifact))
        // ── Gallery ──────────────────────────────────────────────────────
        .route("/gallery", get(gallery::list_gallery_images))
        .route("/gallery/all", get(gallery::list_all_gallery_images))
        .route("/gallery/upload", post(gallery::upload_gallery_image))
        .route("/gallery/:id", delete(gallery::delete_gallery_image))
        // ── Settings ─────────────────────────────────────────────────────
        .route("/settings", get(settings::get_settings).post(settings::save_settings))
        .route("/settings/data-dir", get(settings::get_data_dir))
        // ── Server (llama) ────────────────────────────────────────────────
        .route("/server/status", get(server::get_server_status))
        .route("/server/start", post(server::start_local_server))
        .route("/server/stop", post(server::stop_local_server))
        .route("/server/detect-gpu", get(server::detect_gpu))
        .route("/server/download", post(server::download_llama_server))
        // ── Models ───────────────────────────────────────────────────────
        .route("/models/hf", get(models::list_hf_models))
        .route("/models/downloaded", get(models::list_downloaded_models))
        .route("/models/load", post(models::load_model))
        .route("/models/remote", post(models::connect_remote_server))
        .route("/models/engine-loaded", get(models::is_engine_loaded))
        .route("/models/:id", delete(models::delete_model))
        // ── RAG ──────────────────────────────────────────────────────────
        .route("/rag", get(rag::list_rag_collections).post(rag::create_rag_collection))
        .route("/rag/:id", delete(rag::delete_rag_collection))
        .route("/rag/:id/ingest", post(rag::ingest_document))
        .route("/rag/:id/mode", put(rag::set_retrieval_mode))
        .route("/rag/search", post(rag::search_rag))
        // ── Skills ───────────────────────────────────────────────────────
        .route("/skills/servers", get(skills::list_skill_servers).post(skills::add_mcp_server))
        .route("/skills/servers/:id", delete(skills::remove_mcp_server))
        .route("/skills/tools", get(skills::list_tools))
        .route("/skills/tools/call", post(skills::call_tool_direct))
        .route("/skills/reload-builtins", post(skills::reload_builtin_servers))
        // ── Memory ───────────────────────────────────────────────────────
        .route("/memory", get(memory::list_memory_entries).delete(memory::clear_memory_entries))
        .route("/memory/:id", delete(memory::delete_memory_entry))
        // ── Agents ───────────────────────────────────────────────────────
        .route("/agents", get(agents::list_agent_tasks).post(agents::run_agent_task))
        .route("/agents/:id", delete(agents::delete_agent_task))
        .route("/agents/:id/cancel", post(agents::cancel_agent_task))
        .route("/agents/:id/files", get(agents::list_task_files))
        .route("/agents/:id/files/*path", get(agents::read_task_file))
        // ── ComfyUI Workflows ─────────────────────────────────────────────
        .route("/comfyui/workflows", get(comfyui::list_comfyui_workflows).post(comfyui::save_comfyui_workflow))
        .route("/comfyui/workflows/:id", delete(comfyui::delete_comfyui_workflow))
        // ── Database ─────────────────────────────────────────────────────
        .route("/database/connections", get(database::list_db_connections).post(database::add_db_connection))
        .route("/database/connections/:id", delete(database::delete_db_connection))
        .route("/database/test", post(database::test_db_connection))
        .route("/database/query", post(database::execute_db_query))
        // ── Logs ─────────────────────────────────────────────────────────
        .route("/logs", get(logs::get_logs))
        // Apply auth middleware and state
        .layer(auth_layer)
        .with_state(state);

    Router::new().nest("/api", api)
}
