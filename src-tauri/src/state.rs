use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;

use crate::agent::AgentRuntime;
use crate::db::AppDb;
use crate::engine::EngineManager;
use crate::models::AppSettings;
use crate::rag::RagService;
use crate::server::LlamaServerManager;
use crate::skills::SkillsManager;

/// Central application state shared across Tauri commands.
pub struct AppState {
    /// SQLite app database (sync rusqlite, protected by std Mutex)
    pub db: Arc<Mutex<AppDb>>,
    /// LLM inference engine (local stub + remote OpenAI-compat)
    pub engine: Arc<EngineManager>,
    /// RAG service (async ingest, tokio Mutex so guard is Send across awaits)
    pub rag: Arc<TokioMutex<RagService>>,
    /// ReAct agent runtime
    pub agent_runtime: Arc<AgentRuntime>,
    /// Application settings
    pub settings: Arc<Mutex<AppSettings>>,
    /// Internal llama-server process manager (tokio Mutex so start() can be awaited)
    pub server: Arc<TokioMutex<LlamaServerManager>>,
    /// Skills / MCP tool manager
    pub skills: Arc<SkillsManager>,
    /// Persistent data directory
    pub data_dir: PathBuf,
}

// Safety: all fields are wrapped in Arc<Mutex<...>> or are Send+Sync
unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}
