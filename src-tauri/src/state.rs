use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::broadcast;

use crate::agent::AgentRuntime;
use crate::api_server::events::ApiEvent;
use crate::coding::CodingRuntime;
use crate::db::AppDb;
use crate::engine::EngineManager;
use crate::graph_rag::{GraphRagClient, GraphRagManager};
use crate::models::AppSettings;
use crate::rag::embeddings::Embedder;
use crate::rag::RagService;
use crate::server::LlamaServerManager;
use crate::skills::SkillsManager;
use crate::whisper::WhisperManager;

/// Central application state shared across Tauri commands and HTTP handlers.
pub struct AppState {
    /// SQLite app database (sync rusqlite, protected by std Mutex)
    pub db: Arc<Mutex<AppDb>>,
    /// LLM inference engine (local stub + remote OpenAI-compat)
    pub engine: Arc<EngineManager>,
    /// Embedding client — forwards to llama-server /v1/embeddings (no external deps)
    pub embedder: Arc<Embedder>,
    /// RAG service (async ingest, tokio Mutex so guard is Send across awaits)
    pub rag: Arc<TokioMutex<RagService>>,
    /// ReAct agent runtime
    pub agent_runtime: Arc<AgentRuntime>,
    /// Coding assistant runtime
    pub coding_runtime: Arc<CodingRuntime>,
    /// Application settings
    pub settings: Arc<Mutex<AppSettings>>,
    /// Internal llama-server process manager (tokio Mutex for async start)
    pub server: Arc<TokioMutex<LlamaServerManager>>,
    /// Skills / MCP tool manager
    pub skills: Arc<SkillsManager>,
    /// GraphRAG sidecar process manager
    pub graph_rag: Arc<TokioMutex<GraphRagManager>>,
    /// HTTP client for the graphrag-server (None when sidecar is not running)
    pub graph_rag_client: Option<Arc<GraphRagClient>>,
    /// Whisper speech-to-text sidecar process manager
    pub whisper: Arc<TokioMutex<WhisperManager>>,
    /// Persistent data directory
    pub data_dir: PathBuf,
    /// Broadcast channel for forwarding Tauri events to HTTP/SSE clients
    pub event_tx: broadcast::Sender<ApiEvent>,
    /// Ring buffer of recent log entries for GET /api/logs
    pub log_buffer: Arc<Mutex<VecDeque<serde_json::Value>>>,
    /// Tauri AppHandle — used by HTTP handlers to invoke the same chat pipeline
    pub app_handle: tauri::AppHandle,
}

// Safety: all fields are wrapped in Arc<Mutex<...>> or are Send+Sync
unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

impl Clone for AppState {
    fn clone(&self) -> Self {
        AppState {
            db: self.db.clone(),
            engine: self.engine.clone(),
            embedder: self.embedder.clone(),
            rag: self.rag.clone(),
            agent_runtime: self.agent_runtime.clone(),
            coding_runtime: self.coding_runtime.clone(),
            settings: self.settings.clone(),
            server: self.server.clone(),
            skills: self.skills.clone(),
            graph_rag: self.graph_rag.clone(),
            graph_rag_client: self.graph_rag_client.clone(),
            whisper: self.whisper.clone(),
            data_dir: self.data_dir.clone(),
            event_tx: self.event_tx.clone(),
            log_buffer: self.log_buffer.clone(),
            app_handle: self.app_handle.clone(),
        }
    }
}
