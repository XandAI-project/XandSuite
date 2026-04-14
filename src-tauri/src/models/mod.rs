use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─── Persona ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Base64 data-URL, a single emoji, or None for initials fallback.
    pub avatar: Option<String>,
    pub system_prompt: String,
    /// Preferred model — None means use the app default.
    pub model_id: Option<String>,
    /// JSON-serialised list of RAG collection IDs that are searched by default.
    pub rag_collection_ids: Vec<String>,
    pub memory_enabled: bool,
    /// Auto-created RAG collection used for per-persona memory.
    pub memory_collection_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ─── Chat Models ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub model_id: Option<String>,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub persona_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<Message>,
    /// LLM-generated rolling summary of conversation turns that have been
    /// evicted from the active context window. `None` until compression fires.
    #[serde(default)]
    pub context_summary: Option<String>,
    /// Rowid of the last message already absorbed into `context_summary`.
    /// Used as a watermark for incremental summarization.
    #[serde(default)]
    pub summary_up_to_rowid: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: MessageRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub token_count: Option<u32>,
    pub metadata: Option<serde_json::Value>,
    /// JSON array of tool-call steps recorded when this assistant message was
    /// produced.  `None` for user/system messages or when no tools were used.
    #[serde(default)]
    pub tool_steps: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

// ─── LLM / Engine Models ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub max_tokens: u32,
    pub context_size: u32,
    pub gpu_layers: i32,
    pub repeat_penalty: f32,
    pub stop_sequences: Vec<String>,
    /// Whether to request chain-of-thought reasoning from the model
    pub enable_thinking: bool,
    /// Max tokens for the thinking/reasoning phase (0 = model default)
    pub thinking_budget_tokens: u32,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 2048,
            context_size: 4096,
            gpu_layers: -1,
            repeat_penalty: 1.1,
            stop_sequences: vec![],
            enable_thinking: true,
            thinking_budget_tokens: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum EngineMode {
    Local { model_path: String },
    Remote { server_url: String, api_key: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEvent {
    pub conversation_id: String,
    pub token: String,
    pub done: bool,
}

// ─── HuggingFace / Model Manager Models ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfModel {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
    pub last_modified: Option<String>,
    pub gguf_files: Vec<GgufFile>,
    pub is_downloaded: bool,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GgufFile {
    pub filename: String,
    pub size_bytes: Option<u64>,
    pub quantization: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub filename: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
    Cancelled,
}

// ─── RAG Models ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub collection_id: String,
    pub source_file: String,
    pub content: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RetrievalMode {
    Hybrid,
    Graph,
}

impl Default for RetrievalMode {
    fn default() -> Self { RetrievalMode::Hybrid }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagCollection {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub document_count: u64,
    pub created_at: DateTime<Utc>,
    /// Whether this collection uses hybrid (BM25+cosine) or GraphRAG retrieval.
    #[serde(default)]
    pub retrieval_mode: RetrievalMode,
    /// True once the graphrag-server sidecar has finished indexing all documents.
    #[serde(default)]
    pub graph_indexed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagChunk {
    pub id: String,
    pub document_id: String,
    pub collection_id: String,
    pub content: String,
    pub chunk_index: u32,
    pub metadata: serde_json::Value,
}

// ─── Agent Models ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: AgentTaskStatus,
    pub steps: Vec<AgentStep>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    pub step_number: u32,
    pub thought: String,
    pub action: Option<AgentAction>,
    pub observation: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}

// ─── Flow Models ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowNode {
    pub id: String,
    pub node_type: FlowNodeType,
    pub position_x: f64,
    pub position_y: f64,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FlowNodeType {
    Trigger,
    SystemPrompt,
    UserPrompt,
    TemplatePrompt,
    WebSearch,
    CodeExec,
    DbQuery,
    HttpApi,
    Conditional,
    Loop,
    Merge,
    Input,
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub source_handle: Option<String>,
    pub target_handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowExecution {
    pub id: String,
    pub flow_id: String,
    pub status: FlowExecutionStatus,
    pub node_results: Vec<NodeResult>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FlowExecutionStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResult {
    pub node_id: String,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

// ─── Database Connector Models ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConnection {
    pub id: String,
    pub name: String,
    pub db_type: DbType,
    pub connection_string: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DbType {
    MongoDB,
    PostgreSQL,
    MySQL,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<serde_json::Value>,
    pub row_count: usize,
    pub duration_ms: u64,
}

// ─── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub models_directory: String,
    /// "local" = internal llama-server (default), "remote" = external OpenAI-compat URL
    pub default_engine_mode: String,
    /// Remote server URL — optional, only used when default_engine_mode = "remote"
    pub remote_server_url: Option<String>,
    pub remote_api_key: Option<String>,
    pub hf_api_token: Option<String>,
    pub embedding_model_path: Option<String>,
    pub theme: String,
    pub language: String,
    pub auto_sync_models: bool,
    pub max_agent_iterations: u32,
    pub agent_timeout_seconds: u64,

    // ── Internal llama-server configuration ──────────────────────────────────
    /// Port the internal llama-server listens on
    #[serde(default = "default_server_port")]
    pub llama_server_port: u16,
    /// GPU layers to offload. 0 = CPU only, -1 = all layers on GPU, N = N layers
    #[serde(default)]
    pub n_gpu_layers: i32,
    /// CPU threads (0 = auto-detect)
    #[serde(default)]
    pub server_threads: u32,
    /// Context window size
    #[serde(default = "default_context_size")]
    pub server_context_size: u32,
    /// Batch size for prompt processing
    #[serde(default = "default_batch_size")]
    pub server_batch_size: u32,
    /// Enable flash attention (--flash-attn)
    #[serde(default)]
    pub flash_attention: bool,
    /// Use memory-mapped I/O for model weights
    #[serde(default = "default_true")]
    pub use_mmap: bool,
    /// Last model path loaded into the internal server (auto-updated)
    #[serde(default)]
    pub last_server_model: Option<String>,
    /// Minutes of inactivity before the server is automatically stopped to free
    /// VRAM.  0 = keep running forever.  Default: 5.
    #[serde(default = "default_keep_alive_mins")]
    pub model_keep_alive_mins: u32,

    // ── Reasoning / Thinking ─────────────────────────────────────────────────
    /// How llama-server surfaces chain-of-thought tokens.
    /// "none" = off, "generic" = <think> tags (Qwen3/general), "deepseek" = DeepSeek-R1 format.
    #[serde(default = "default_reasoning_format")]
    pub reasoning_format: String,
    /// Enable thinking/chain-of-thought in responses for models that support it.
    #[serde(default = "default_true")]
    pub enable_thinking: bool,
    /// Max tokens the model may spend on chain-of-thought.  0 = model decides (can overthink).
    #[serde(default = "default_thinking_budget")]
    pub thinking_budget_tokens: u32,
    /// Max tokens the model may generate for the visible response (not counting thinking).
    /// Effective server limit = thinking_budget_tokens + max_response_tokens.
    #[serde(default = "default_max_response_tokens")]
    pub max_response_tokens: u32,

    // ── Code Execution ───────────────────────────────────────────────────────
    /// Allow the LLM to execute code in a sandboxed subprocess and see the output.
    /// Requires Python 3 / Node.js to be installed for those languages.
    #[serde(default)]
    pub enable_code_execution: bool,

    // ── VLM / Multimodal ─────────────────────────────────────────────────────
    /// Path to the multimodal projection file (mmproj-*.gguf) for VLM models.
    /// When set, passed as `--mmproj <path>` to llama-server on startup.
    /// Clear this when loading a non-VLM model to avoid passing an incompatible file.
    #[serde(default)]
    pub mmproj_path: Option<String>,

    // ── Memory ───────────────────────────────────────────────────────────────
    /// Automatically extract key facts from chat exchanges and recall them in
    /// future conversations.  Stored in the internal RAG memory collection.
    #[serde(default = "default_true")]
    pub memory_enabled: bool,

    // ── Mobile API Bridge ─────────────────────────────────────────────────────
    /// Enable the embedded HTTP/SSE bridge server for the mobile frontend.
    #[serde(default)]
    pub mobile_api_enabled: bool,
    /// Port for the mobile HTTP bridge (default 3847).
    #[serde(default = "default_mobile_api_port")]
    pub mobile_api_port: u16,
    /// Optional secret token for API auth. Empty = no authentication required.
    #[serde(default)]
    pub mobile_api_token: Option<String>,

    // ── Knowledge Base / RAG ─────────────────────────────────────────────────
    /// fastembed model name used for chunk embeddings.
    /// Valid values: "nomic-embed-text-v1.5" (default), "all-MiniLM-L6-v2",
    /// "bge-base-en-v1.5", "bge-large-en-v1.5", "bge-small-en-v1.5", "bge-m3",
    /// "nomic-embed-text-v1.5-quantized", "all-MiniLM-L6-v2-quantized".
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    /// Weight given to the cosine similarity component in hybrid search (0–1).
    /// The BM25 component receives (1 - hybrid_cosine_weight).
    /// Default: 0.6 (slightly prefer semantic over keyword).
    #[serde(default = "default_hybrid_cosine_weight")]
    pub hybrid_cosine_weight: f32,

    // ── GraphRAG sidecar ─────────────────────────────────────────────────────
    /// Enable the optional graphrag-server sidecar process.
    #[serde(default)]
    pub graph_rag_enabled: bool,
    /// Port for the graphrag-server sidecar (default 3848).
    #[serde(default = "default_graph_rag_port")]
    pub graph_rag_port: u16,
    /// Override path to the graphrag-server binary. If None the binary is
    /// looked up in <data_dir>/graphrag-server[.exe].
    #[serde(default)]
    pub graph_rag_server_path: Option<String>,
    /// Auto-start graphrag-server together with the app when graph_rag_enabled.
    #[serde(default)]
    pub graph_rag_auto_start: bool,
    /// Vector database backend for graphrag-server: "lancedb" (default,
    /// embedded, no extra process) or "qdrant".
    #[serde(default = "default_graph_rag_vector_db")]
    pub graph_rag_vector_db: String,

    // ── User profile (collected during onboarding) ───────────────────────────
    /// Set to true once the first-launch onboarding wizard is completed.
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub user_profession: Option<String>,
    #[serde(default)]
    pub user_about: Option<String>,

    // ── Voice input (whisper.cpp) ─────────────────────────────────────────────
    /// Show the microphone button in the chat input bar.
    #[serde(default)]
    pub whisper_enabled: bool,
    /// Path to the downloaded ggml model file (e.g. ggml-base.bin).
    #[serde(default)]
    pub whisper_model_path: Option<String>,
    /// BCP-47 language code passed to whisper-server, or "auto" for detection.
    #[serde(default = "default_whisper_language")]
    pub whisper_language: String,
    /// Port the whisper-server sidecar listens on.
    #[serde(default = "default_whisper_port")]
    pub whisper_port: u16,
    /// Which whisper-server build to download: "cpu" or "cuda".
    #[serde(default = "default_whisper_variant")]
    pub whisper_variant: String,

    // ── Voice output (KokoroTTS) ──────────────────────────────────────────────
    /// Enable KokoroTTS voice output and the voice-to-voice conversation mode.
    #[serde(default)]
    pub tts_enabled: bool,
    /// Port the kokoro_server.py sidecar listens on.
    #[serde(default = "default_tts_port")]
    pub tts_port: u16,
    /// Selected voice ID (e.g. "af_heart", "pf_dora").
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    /// Speech rate multiplier (1.0 = normal speed).
    #[serde(default = "default_tts_speed")]
    pub tts_speed: f32,
    /// BCP-47 language code for TTS: "en-us", "pt-br", "es", "fr", etc.
    #[serde(default = "default_tts_language")]
    pub tts_language: String,
    /// Torch device for KokoroTTS: "cpu", "cuda11", or "cuda12".
    #[serde(default = "default_tts_device")]
    pub tts_device: String,
}

/// Reserved collection ID for the auto-generated internal memory.
pub const MEMORY_COLLECTION_ID: &str = "xand_internal_memory";

fn default_keep_alive_mins() -> u32 { 5 }
fn default_reasoning_format() -> String { "deepseek".to_string() }
fn default_thinking_budget() -> u32 { 1024 }
fn default_max_response_tokens() -> u32 { 2048 }

fn default_mobile_api_port() -> u16 { 3847 }
fn default_server_port() -> u16 { 11434 }
fn default_context_size() -> u32 { 4096 }
fn default_batch_size() -> u32 { 512 }
fn default_true() -> bool { true }
fn default_embedding_model() -> String { "nomic-embed-text-v1.5".to_string() }
fn default_hybrid_cosine_weight() -> f32 { 0.6 }
fn default_graph_rag_port() -> u16 { 3848 }
fn default_graph_rag_vector_db() -> String { "lancedb".to_string() }
fn default_whisper_language() -> String { "auto".to_string() }
fn default_whisper_port() -> u16 { 8765 }
fn default_whisper_variant() -> String { "cpu".to_string() }

fn default_tts_port() -> u16 { 8766 }
fn default_tts_voice() -> String { "af_heart".to_string() }
fn default_tts_speed() -> f32 { 1.0 }
fn default_tts_language() -> String { "en-us".to_string() }
fn default_tts_device() -> String { "cpu".to_string() }

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            models_directory: String::from("models"),
            default_engine_mode: String::from("local"),
            remote_server_url: None,
            remote_api_key: None,
            hf_api_token: None,
            embedding_model_path: None,
            theme: String::from("dark"),
            language: String::from("en"),
            auto_sync_models: true,
            max_agent_iterations: 10,
            agent_timeout_seconds: 300,
            llama_server_port: 11434,
            n_gpu_layers: 0,
            server_threads: 0,
            server_context_size: 4096,
            server_batch_size: 512,
            flash_attention: false,
            use_mmap: true,
            last_server_model: None,
            model_keep_alive_mins: 5,
            reasoning_format: "deepseek".to_string(),
            enable_thinking: true,
            thinking_budget_tokens: 1024,
            max_response_tokens: 2048,
            enable_code_execution: false,
            mmproj_path: None,
            memory_enabled: true,
            mobile_api_enabled: false,
            mobile_api_port: 3847,
            mobile_api_token: None,
            embedding_model: "nomic-embed-text-v1.5".to_string(),
            hybrid_cosine_weight: 0.6,
            graph_rag_enabled: false,
            graph_rag_port: 3848,
            graph_rag_server_path: None,
            graph_rag_auto_start: false,
            graph_rag_vector_db: "lancedb".to_string(),
            onboarding_completed: false,
            user_name: None,
            user_profession: None,
            user_about: None,
            whisper_enabled: false,
            whisper_model_path: None,
            whisper_language: "auto".to_string(),
            whisper_port: 8765,
            whisper_variant: "cpu".to_string(),
            tts_enabled: false,
            tts_port: 8766,
            tts_voice: "af_heart".to_string(),
            tts_speed: 1.0,
            tts_language: "en-us".to_string(),
            tts_device: "cpu".to_string(),
        }
    }
}
