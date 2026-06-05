use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::broadcast;

use crate::agent::AgentRuntime;
use crate::agent_browser::cookie_vault::CookieVault;
use crate::agent_browser::profile::ProfileManager;
use crate::agent_browser::safety::SafetyGate;
use crate::agent_browser::session::BrowserSessionRegistry;
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
use crate::tts::KokoroManager;
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
    /// KokoroTTS text-to-speech sidecar process manager
    pub tts: Arc<TokioMutex<KokoroManager>>,
    /// Persistent data directory
    pub data_dir: PathBuf,
    /// Broadcast channel for forwarding Tauri events to HTTP/SSE clients
    pub event_tx: broadcast::Sender<ApiEvent>,
    /// Ring buffer of recent log entries for GET /api/logs
    pub log_buffer: Arc<Mutex<VecDeque<serde_json::Value>>>,
    /// Tauri AppHandle — used by HTTP handlers to invoke the same chat pipeline
    pub app_handle: tauri::AppHandle,
    /// Set to true by `stop_generation`; the streaming loop checks this to abort early.
    pub generation_cancelled: Arc<AtomicBool>,
    /// Set to true while a MCP/skills tool call is being dispatched.
    /// The idle-watcher uses this to avoid killing the LLM server while a
    /// long-running tool (e.g. video generation) is still in progress.
    pub tool_active: Arc<AtomicBool>,
    /// Registry of active Browser Agent sessions keyed by conversation id.
    pub browser_sessions: Arc<BrowserSessionRegistry>,
    /// Per-app browser profile manager (disposable / named user-data-dirs).
    pub browser_profiles: Arc<ProfileManager>,
    /// Shared safety gate for browser-agent actions.
    pub browser_safety: Arc<SafetyGate>,
    /// Disk-backed vault of pasted cookie sessions (LinkedIn, Gmail, …).
    pub browser_cookie_vault: Arc<CookieVault>,
}

impl AppState {
    /// Whether the LLM is allowed to call `browser_agent__start_session`
    /// on its own. Persisted in the `settings` table under the key
    /// `browser_agent_autostart`; defaults to `true` because the feature was
    /// introduced opt-out (the toggle lives in Settings → Browser).
    ///
    /// Reads are cheap (one SQLite lookup through `AppDb::get_setting`) and
    /// happen once per `start_session` tool call, so we don't bother caching.
    pub fn browser_agent_autostart_allowed(&self) -> bool {
        let db = match self.db.lock() {
            Ok(guard) => guard,
            // Poisoned mutex: fail closed — no autostart until the user
            // restarts the app and we can read the setting cleanly again.
            Err(_) => return false,
        };
        match db.get_setting("browser_agent_autostart") {
            Ok(Some(v)) => v != "0" && v.to_ascii_lowercase() != "false",
            Ok(None) => true,
            Err(_) => true,
        }
    }
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
            tts: self.tts.clone(),
            data_dir: self.data_dir.clone(),
            event_tx: self.event_tx.clone(),
            log_buffer: self.log_buffer.clone(),
            app_handle: self.app_handle.clone(),
            generation_cancelled: self.generation_cancelled.clone(),
            tool_active: self.tool_active.clone(),
            browser_sessions: self.browser_sessions.clone(),
            browser_profiles: self.browser_profiles.clone(),
            browser_safety: self.browser_safety.clone(),
            browser_cookie_vault: self.browser_cookie_vault.clone(),
        }
    }
}
