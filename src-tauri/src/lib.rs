#![allow(dead_code)]

mod agent;
mod api_server;
mod code_runner;
mod coding;
mod commands;
mod db;
mod engine;
mod flow;
mod graph_rag;
mod hf;
mod jobs;
mod models;
mod process_ext;
mod rag;
mod server;
mod skills;
mod skills_init;
mod state;
mod tts;
mod web_fetch;
mod whisper;

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::sync::Mutex as TokioMutex;
use tauri::Manager;

use crate::agent::AgentRuntime;
use crate::coding::CodingRuntime;
use crate::db::AppDb;
use crate::engine::EngineManager;
use crate::graph_rag::{GraphRagClient, GraphRagManager};
use crate::models::AppSettings;
use crate::rag::embeddings::Embedder;
use crate::rag::RagService;
use crate::server::LlamaServerManager;
use crate::skills::SkillsManager;
use crate::state::AppState;
use crate::tts::KokoroManager;
use crate::whisper::WhisperManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            env_logger::init();

            let data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data directory");

            std::fs::create_dir_all(&data_dir)
                .expect("Failed to create app data directory");
            std::fs::create_dir_all(data_dir.join("cache"))
                .expect("Failed to create cache directory");
            std::fs::create_dir_all(data_dir.join("models"))
                .expect("Failed to create models directory");
            std::fs::create_dir_all(data_dir.join("agent_workspace"))
                .expect("Failed to create agent workspace directory");

            // Initialize SQLite
            let db = AppDb::open(&data_dir).expect("Failed to initialize database");
            let db = Arc::new(Mutex::new(db));

            // Load settings from DB
            let settings: AppSettings = {
                let db_guard = db.lock().unwrap();
                match db_guard.get_setting("app_settings") {
                    Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
                    _ => AppSettings::default(),
                }
            };
            let settings = Arc::new(Mutex::new(settings));

            // Initialize engine
            let engine = Arc::new(EngineManager::new());

            // Embedder delegates to the running llama-server /v1/embeddings endpoint.
            // No heavy ONNX/fastembed dependency — zero startup cost.
            let embedder = Arc::new(Embedder::new(settings.clone()));

            // Initialize RAG service (uses tokio Mutex so guard is Send across awaits)
            let rag = RagService::new(db.clone(), &data_dir)
                .expect("Failed to initialize RAG service");
            let rag = Arc::new(TokioMutex::new(rag));

            // Initialize agent runtime (shares the same DB Arc)
            let workspace_dir = data_dir.join("agent_workspace");
            let (max_iter, timeout_secs) = {
                let s = settings.lock().unwrap();
                (s.max_agent_iterations, s.agent_timeout_seconds)
            };
            let agent_runtime = Arc::new(AgentRuntime::new(
                db.clone(),
                workspace_dir,
                max_iter,
                timeout_secs,
            ));

            // Initialize coding runtime
            let coding_runtime = Arc::new(CodingRuntime::new(
                db.clone(),
                max_iter,
                timeout_secs,
            ));

            // Spawn background HF sync job
            let cache_dir = data_dir.join("cache");
            let api_token = settings.lock().unwrap().hf_api_token.clone();
            tauri::async_runtime::spawn(async move {
                // Delay to let the app start
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                jobs::hf_sync::run_hf_sync_loop(cache_dir, api_token).await;
            });

            // Initialize internal llama-server manager (tokio Mutex for async start)
            let server = Arc::new(TokioMutex::new(LlamaServerManager::new()));

            // Shared flag: true while an MCP/skills tool call is being dispatched.
            let tool_active: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

            // ── Background idle-watcher ──────────────────────────────────────
            // Checks every 60 s whether the server has been idle longer than
            // model_keep_alive_mins; stops it automatically to free VRAM.
            // A keep_alive_mins of 0 disables auto-stop.
            // When a tool is actively running (tool_active == true), the watcher
            // resets the idle timer instead of killing the server so that long
            // external processes (e.g. ComfyUI video generation) don't cause the
            // next LLM call to fail with "Failed to connect to LLM server".
            {
                let server_arc = server.clone();
                let settings_arc = settings.clone();
                let tool_active_arc = tool_active.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                        let keep_alive = {
                            settings_arc.lock().unwrap().model_keep_alive_mins
                        };
                        if keep_alive == 0 {
                            continue;
                        }
                        let mut srv = server_arc.lock().await;
                        // If a tool is currently executing, reset the idle timer so the
                        // server is kept alive until the tool finishes.
                        if tool_active_arc.load(std::sync::atomic::Ordering::Relaxed) {
                            srv.touch();
                            continue;
                        }
                        if srv.is_running() && srv.is_idle(keep_alive) {
                            log::info!(
                                "llama-server idle for {} min — stopping to free VRAM.",
                                keep_alive
                            );
                            srv.stop();
                        }
                    }
                });
            }

            // Initialize GraphRAG sidecar manager
            let graph_rag = Arc::new(TokioMutex::new(GraphRagManager::new()));

            // Determine GraphRAG startup settings
            let (gr_enabled, gr_auto_start, gr_port, gr_vector_db, gr_server_path) = {
                let s = settings.lock().unwrap();
                (
                    s.graph_rag_enabled,
                    s.graph_rag_auto_start,
                    s.graph_rag_port,
                    s.graph_rag_vector_db.clone(),
                    s.graph_rag_server_path.clone(),
                )
            };

            let graph_rag_client: Option<Arc<GraphRagClient>> = if gr_enabled && gr_auto_start {
                let data_dir_gr = data_dir.clone();
                let embedding_model_name_gr = settings.lock().unwrap().embedding_model.clone();
                let gr_arc = graph_rag.clone();

                tauri::async_runtime::spawn(async move {
                    let mut mgr = gr_arc.lock().await;
                    if let Err(e) = mgr.start(
                        &data_dir_gr,
                        gr_port,
                        &gr_vector_db,
                        &embedding_model_name_gr,
                        gr_server_path.as_deref(),
                    ) {
                        log::warn!("GraphRAG auto-start failed: {}", e);
                    } else if let Err(e) = mgr.wait_ready(30).await {
                        log::warn!("GraphRAG did not become ready: {}", e);
                    }
                });

                Some(Arc::new(GraphRagClient::new(gr_port)))
            } else if gr_enabled {
                Some(Arc::new(GraphRagClient::new(gr_port)))
            } else {
                None
            };

            // Initialize SkillsManager
            let workspace_dir = data_dir.join("agent_workspace");
            let tools_dir = {
                let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
                std::path::PathBuf::from(&manifest)
                    .parent()
                    .unwrap_or(&std::path::PathBuf::from("."))
                    .join("tools")
            };
            let skills = Arc::new(SkillsManager::new(tools_dir, workspace_dir.clone()));

            // Spawn background task to connect builtin + user MCP servers.
            // Extract user server configs synchronously (no Send issues) before spawning.
            {
                let skills_arc = skills.clone();
                let data_dir_arc = data_dir.clone();

                // Collect user-added servers synchronously; the lock is dropped before spawn.
                let user_server_configs: Vec<crate::skills::McpServerConfig> = {
                    let db_guard = db.lock().unwrap();
                    db_guard
                        .get_setting("mcp_servers")
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default()
                };

                tauri::async_runtime::spawn(async move {
                    // Brief delay so the UI has time to paint
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    if let Err(e) = skills_init::connect_builtin_servers(&skills_arc, &data_dir_arc).await {
                        log::warn!("Builtin MCP servers init error: {}", e);
                    }
                    for cfg in user_server_configs {
                        if let Err(e) = skills_arc.connect_server(cfg.clone()).await {
                            log::warn!("Failed to reconnect user MCP server '{}': {}", cfg.id, e);
                        }
                    }
                });
            }

            // Create broadcast channel for HTTP/SSE event bridge
            let (event_tx, _) = broadcast::channel::<crate::api_server::events::ApiEvent>(1024);
            let log_buffer = Arc::new(Mutex::new(VecDeque::<serde_json::Value>::new()));
            let app_handle = app.handle().clone();

            // Determine whether to spawn the mobile API server
            let mobile_api_enabled = settings.lock().unwrap().mobile_api_enabled;
            let mobile_api_port = settings.lock().unwrap().mobile_api_port;

            // Whisper sidecar manager
            let whisper = Arc::new(TokioMutex::new(WhisperManager::new()));

            // KokoroTTS sidecar manager
            let tts = Arc::new(TokioMutex::new(KokoroManager::new()));

            let app_state = AppState {
                db,
                engine,
                embedder,
                rag,
                agent_runtime,
                coding_runtime,
                settings,
                server,
                skills,
                graph_rag,
                graph_rag_client,
                whisper,
                tts,
                data_dir,
                event_tx,
                log_buffer,
                app_handle,
                generation_cancelled: Arc::new(AtomicBool::new(false)),
                tool_active,
            };

            // Create an Arc<AppState> for the HTTP server (shares all Arc fields)
            let state_arc: Arc<AppState> = Arc::new(app_state.clone());

            // Register the plain AppState for existing Tauri commands (State<'_, AppState>)
            app.manage(app_state);
            // Also manage Arc<AppState> so HTTP handlers can retrieve it via app.state::<Arc<AppState>>()
            app.manage(state_arc.clone());

            let headless = std::env::var("XANDSUITE_HEADLESS").is_ok();

            // In headless mode the API server is always started regardless of settings.
            // In desktop mode respect the persisted mobile_api_enabled toggle.
            if headless || mobile_api_enabled {
                let arc_for_task = state_arc.clone();
                let port = mobile_api_port;
                tauri::async_runtime::spawn(async move {
                    crate::api_server::start_api_server(arc_for_task, port).await;
                });
            }

            // In desktop (non-headless) mode create the main application window.
            if !headless {
                tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("XandSuite")
                .inner_size(1280.0, 800.0)
                .min_inner_size(900.0, 600.0)
                .decorations(true)
                .build()
                .expect("Failed to create main window");
            }

            // Reconnect previously installed packages (after a small delay so
            // builtin servers can register first).
            {
                let arc_for_pkgs = state_arc.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    crate::commands::packages::reconnect_installed_packages(&arc_for_pkgs).await;
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::chat::list_conversations,
            commands::chat::create_conversation,
            commands::chat::update_conversation,
            commands::chat::get_conversation,
            commands::chat::delete_conversation,
            commands::chat::rename_conversation,
            commands::chat::truncate_conversation,
            commands::chat::send_message,
            commands::chat::save_message_tool_steps,
            commands::chat::stop_generation,
            commands::models::list_hf_models,
            commands::models::refresh_hf_models,
            commands::models::download_model,
            commands::models::list_downloaded_models,
            commands::models::delete_model,
            commands::models::load_model,
            commands::models::connect_remote_server,
            commands::models::is_engine_loaded,
            commands::rag::list_rag_collections,
            commands::rag::create_rag_collection,
            commands::rag::delete_rag_collection,
            commands::rag::ingest_document,
            commands::rag::search_rag,
            commands::rag::set_collection_retrieval_mode,
            commands::rag::reindex_collection,
            graph_rag::commands::graph_rag_status,
            graph_rag::commands::start_graph_rag,
            graph_rag::commands::stop_graph_rag,
            graph_rag::commands::ingest_to_graph,
            graph_rag::commands::query_graph,
            commands::agents::run_agent_task,
            commands::agents::list_agent_tasks,
            commands::agents::delete_agent_task,
            commands::agents::cancel_agent_task,
            commands::agents::list_task_files,
            commands::agents::read_task_file,
            commands::agents::open_task_workspace,
            commands::flows::list_flows,
            commands::flows::save_flow,
            commands::flows::delete_flow,
            commands::flows::execute_flow,
            commands::database::list_db_connections,
            commands::database::add_db_connection,
            commands::database::delete_db_connection,
            commands::database::execute_db_query,
            commands::database::test_db_connection,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::get_data_dir,
            commands::settings::get_models_dir,
            commands::server::get_server_status,
            commands::server::start_local_server,
            commands::server::stop_local_server,
            commands::server::download_llama_server,
            commands::server::detect_gpu,
            commands::server::touch_server,
            commands::server::ensure_server_running,
            commands::skills::list_tools,
            commands::skills::list_skill_servers,
            commands::skills::add_mcp_server,
            commands::skills::remove_mcp_server,
            commands::skills::reload_builtin_servers,
            commands::skills::call_tool_direct,
            commands::artifacts::save_artifact,
            commands::artifacts::list_artifacts,
            commands::artifacts::list_all_artifacts,
            commands::artifacts::delete_artifact,
            commands::artifacts::update_artifact,
            commands::attachments::read_attachment,
            commands::attachments::read_file_as_base64,
            commands::coding::create_coding_session,
            commands::coding::list_coding_sessions,
            commands::coding::get_coding_session,
            commands::coding::update_coding_session,
            commands::coding::delete_coding_session,
            commands::coding::send_coding_message,
            commands::coding::cancel_coding_session,
            commands::coding::select_coding_project,
            commands::coding::list_coding_directory,
            commands::coding::read_coding_file,
            commands::coding::get_coding_plan,
            commands::memory::list_memory_entries,
            commands::memory::delete_memory_entry,
            commands::memory::clear_memory_entries,
            commands::gallery::list_gallery_images,
            commands::gallery::list_all_gallery_images,
            commands::gallery::delete_gallery_image,
            commands::gallery::save_upload_to_gallery,
            commands::whisper::get_whisper_status,
            commands::whisper::start_whisper_server,
            commands::whisper::stop_whisper_server,
            commands::whisper::transcribe_audio,
            commands::whisper::download_whisper_binary,
            commands::whisper::download_whisper_model,
            commands::personas::list_personas,
            commands::personas::get_persona,
            commands::personas::create_persona,
            commands::personas::update_persona,
            commands::personas::delete_persona,
            commands::templates::list_templates,
            commands::templates::create_template,
            commands::templates::update_template,
            commands::templates::delete_template,
            commands::templates::increment_template_use,
            commands::packages::list_official_packages,
            commands::packages::install_package,
            commands::packages::uninstall_package,
            commands::packages::list_custom_packages,
            commands::packages::save_custom_package,
            commands::packages::get_custom_package_code,
            commands::packages::delete_custom_package,
            commands::packages::install_custom_package,
            commands::packages::uninstall_custom_package,
            commands::packages::fetch_comfyui_workflows,
            commands::tts::get_tts_status,
            commands::tts::start_tts_server,
            commands::tts::stop_tts_server,
            commands::tts::synthesize_speech,
            commands::tts::download_tts_models,
            commands::tts::get_tts_log,
        ])
        .build(tauri::generate_context!())
        .expect("error building XandSuite")
        .run(|_app_handle, event| {
            // In headless mode prevent the process from exiting when there are
            // no open windows — the Axum server keeps the runtime alive.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if std::env::var("XANDSUITE_HEADLESS").is_ok() {
                    api.prevent_exit();
                }
            }
        });
}
