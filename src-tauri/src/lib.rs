#![allow(dead_code)]

mod agent;
mod code_runner;
mod coding;
mod commands;
mod db;
mod engine;
mod flow;
mod hf;
mod jobs;
mod models;
mod rag;
mod server;
mod skills;
mod skills_init;
mod state;
mod web_fetch;

use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;
use tauri::Manager;

use crate::agent::AgentRuntime;
use crate::coding::CodingRuntime;
use crate::db::AppDb;
use crate::engine::EngineManager;
use crate::models::AppSettings;
use crate::rag::RagService;
use crate::server::LlamaServerManager;
use crate::skills::SkillsManager;
use crate::state::AppState;

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

            // ── Background idle-watcher ──────────────────────────────────────
            // Checks every 60 s whether the server has been idle longer than
            // model_keep_alive_mins; stops it automatically to free VRAM.
            // A keep_alive_mins of 0 disables auto-stop.
            {
                let server_arc = server.clone();
                let settings_arc = settings.clone();
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

            // Register app state
            app.manage(AppState {
                db,
                engine,
                rag,
                agent_runtime,
                coding_runtime,
                settings,
                server,
                skills,
                data_dir,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::chat::list_conversations,
            commands::chat::create_conversation,
            commands::chat::update_conversation,
            commands::chat::get_conversation,
            commands::chat::delete_conversation,
            commands::chat::truncate_conversation,
            commands::chat::send_message,
            commands::chat::save_message_tool_steps,
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
            commands::comfyui::list_comfyui_workflows,
            commands::comfyui::save_comfyui_workflow,
            commands::comfyui::delete_comfyui_workflow,
            commands::gallery::list_gallery_images,
            commands::gallery::list_all_gallery_images,
            commands::gallery::delete_gallery_image,
            commands::gallery::save_upload_to_gallery,
        ])
        .run(tauri::generate_context!())
        .expect("error while running XandSuite");
}
