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
    log::info!("API server listening on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("Failed to bind API server on {}: {}", addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        log::error!("API server error: {}", e);
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
        .route("/chat/stop", post(stop::stop_generation))
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
        .route("/models/hf/refresh", post(misc::refresh_hf_models))
        .route("/models/downloaded", get(models::list_downloaded_models))
        .route("/models/load", post(models::load_model))
        .route("/models/remote", post(models::connect_remote_server))
        .route("/models/engine-loaded", get(models::is_engine_loaded))
        .route("/models/dir", get(misc::get_models_dir))
        .route("/models/:id", delete(models::delete_model))
        // ── RAG ──────────────────────────────────────────────────────────
        .route("/rag", get(rag::list_rag_collections).post(rag::create_rag_collection))
        .route("/rag/:id", delete(rag::delete_rag_collection))
        .route("/rag/:id/ingest", post(rag::ingest_document))
        .route("/rag/:id/mode", put(rag::set_retrieval_mode))
        .route("/rag/:id/reindex", post(misc::reindex_collection))
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
        .route("/agents/:id/open-workspace", post(misc::open_task_workspace))
        // ── Flows ─────────────────────────────────────────────────────────
        .route("/flows", get(flows::list_flows).post(flows::save_flow))
        .route("/flows/:id", delete(flows::delete_flow))
        .route("/flows/:id/execute", post(flows::execute_flow))
        // ── ComfyUI Workflows ─────────────────────────────────────────────
        .route("/comfyui/workflows", get(comfyui::list_comfyui_workflows).post(comfyui::save_comfyui_workflow))
        .route("/comfyui/workflows/:id", delete(comfyui::delete_comfyui_workflow))
        .route("/comfyui/fetch", post(misc::fetch_comfyui_workflows))
        // ── Database ─────────────────────────────────────────────────────
        .route("/database/connections", get(database::list_db_connections).post(database::add_db_connection))
        .route("/database/connections/:id", delete(database::delete_db_connection))
        .route("/database/test", post(database::test_db_connection))
        .route("/database/query", post(database::execute_db_query))
        // ── Personas ─────────────────────────────────────────────────────
        .route("/personas", get(personas::list_personas).post(personas::create_persona))
        .route("/personas/:id", get(personas::get_persona).put(personas::update_persona).delete(personas::delete_persona))
        // ── Templates ────────────────────────────────────────────────────
        .route("/templates", get(templates::list_templates).post(templates::create_template))
        .route("/templates/:id", put(templates::update_template).delete(templates::delete_template))
        .route("/templates/:id/use", post(templates::increment_template_use))
        // ── Packages ─────────────────────────────────────────────────────
        .route("/packages/official", get(packages::list_official_packages))
        .route("/packages/official/:id/install", post(packages::install_package))
        .route("/packages/official/:id", delete(packages::uninstall_package))
        .route("/packages/custom", get(packages::list_custom_packages).post(packages::save_custom_package))
        .route("/packages/custom/:id/code", get(packages::get_custom_package_code))
        .route("/packages/custom/:id/install", post(packages::install_custom_package))
        .route("/packages/custom/:id/uninstall", post(packages::uninstall_custom_package))
        .route("/packages/custom/:id", delete(packages::delete_custom_package))
        // ── Whisper ──────────────────────────────────────────────────────
        .route("/whisper/status", get(whisper::get_whisper_status))
        .route("/whisper/start", post(whisper::start_whisper_server))
        .route("/whisper/stop", post(whisper::stop_whisper_server))
        .route("/whisper/transcribe", post(whisper::transcribe_audio))
        .route("/whisper/download-binary", post(whisper::download_whisper_binary))
        .route("/whisper/download-model", post(whisper::download_whisper_model))
        // ── TTS (KokoroTTS) ──────────────────────────────────────────────────
        .route("/tts/status", get(tts::get_tts_status))
        .route("/tts/start", post(tts::start_tts_server))
        .route("/tts/stop", post(tts::stop_tts_server))
        .route("/tts/synthesize", post(tts::synthesize_speech))
        .route("/tts/download-models", post(tts::download_tts_models))
        .route("/tts/log", get(tts::get_tts_log))
        // ── Attachments / file reading ────────────────────────────────────
        .route("/files/base64", post(misc::read_file_as_base64))
        // ── Logs ─────────────────────────────────────────────────────────
        .route("/logs", get(logs::get_logs))
        // Apply auth middleware and state
        .layer(auth_layer)
        .with_state(state.clone());

    // Public image-hosting routes — no auth, so the Python packages and ComfyUI
    // can fetch/upload images using only the local API port (works fully offline).
    let images = Router::new()
        .route("/images/:id", get(gallery::serve_gallery_image))
        .route("/images/upload", post(gallery::upload_image_public))
        .with_state(state.clone());

    // Public download routes — serve installer binaries without auth.
    let downloads = Router::new()
        .route("/api/download", get(download::list_installers))
        .route("/api/download/auto", get(download::download_auto))
        .route("/api/download/:filename", get(download::download_file));

    // Static frontend files — served from the `dist/` directory next to the binary.
    // In headless/server mode the built React SPA is embedded here.
    // Fallback to index.html for client-side routing (SPA mode).
    let frontend_dist = resolve_frontend_dist();
    let static_files = if frontend_dist.exists() {
        log::info!("Serving frontend from {:?}", frontend_dist);
        Some(
            Router::new().fallback_service(
                tower_http::services::ServeDir::new(&frontend_dist)
                    .fallback(tower_http::services::ServeFile::new(
                        frontend_dist.join("index.html"),
                    )),
            ),
        )
    } else {
        log::info!("No frontend dist directory found at {:?} — static serving disabled", frontend_dist);
        None
    };

    let mut router = Router::new()
        .nest("/api", api)
        .merge(images)
        .merge(downloads);

    if let Some(sf) = static_files {
        router = router.merge(sf);
    }

    router
}

/// Resolve the frontend `dist/` directory.
/// Checks (in order):
///   1. `XANDSUITE_FRONTEND_DIST` env var
///   2. `dist/` adjacent to the running binary
///   3. `../dist/` relative to the binary (dev layout)
fn resolve_frontend_dist() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("XANDSUITE_FRONTEND_DIST") {
        return std::path::PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        let adjacent = exe.parent().unwrap_or(&exe).join("dist");
        if adjacent.exists() {
            return adjacent;
        }
        let up_one = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("dist"))
            .unwrap_or_else(|| adjacent.clone());
        if up_one.exists() {
            return up_one;
        }
    }
    std::path::PathBuf::from("dist")
}
