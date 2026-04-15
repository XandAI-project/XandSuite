// Re-export invoke and listen from the unified transport layer.
// In Tauri mode: delegates to @tauri-apps/api (native IPC).
// In web/headless mode: delegates to HTTP fetch + SSE EventSource.
export { invoke, listen, isTauri } from "./transport";
export type { UnlistenFn } from "./transport";

// Type-safe wrappers for Tauri commands

// ── Prompt Templates ─────────────────────────────────────────────────────────

export interface PromptTemplate {
  id: string;
  title: string;
  content: string;
  description?: string;
  category?: string;
  /** Optional slash-command shortcut, e.g. "/sum". */
  shortcut?: string;
  /** Optional package name required to use this template, e.g. "ComfyUI Images". */
  requires?: string;
  use_count: number;
  created_at: string;
  updated_at: string;
}

export interface CreateTemplateInput {
  title: string;
  content: string;
  description?: string;
  category?: string;
  shortcut?: string;
  requires?: string;
}

export interface UpdateTemplateInput {
  id: string;
  title?: string;
  content?: string;
  description?: string;
  category?: string;
  shortcut?: string;
  requires?: string;
}

// ── Personas ────────────────────────────────────────────────────────────────

export interface Persona {
  id: string;
  name: string;
  description?: string;
  /** Base64 data-URL, a single emoji, or undefined for initials fallback. */
  avatar?: string;
  system_prompt: string;
  /** Preferred model path/id — undefined means use the app default. */
  model_id?: string;
  rag_collection_ids: string[];
  memory_enabled: boolean;
  /** Auto-created RAG collection for per-persona memory. */
  memory_collection_id?: string;
  created_at: string;
  updated_at: string;
}

export interface CreatePersonaInput {
  name: string;
  description?: string;
  avatar?: string;
  system_prompt: string;
  model_id?: string;
  rag_collection_ids: string[];
  memory_enabled: boolean;
}

export interface UpdatePersonaInput {
  id: string;
  name?: string;
  description?: string;
  avatar?: string;
  system_prompt?: string;
  model_id?: string;
  rag_collection_ids?: string[];
  memory_enabled?: boolean;
}

// ── Conversations ────────────────────────────────────────────────────────────

export interface Conversation {
  id: string;
  title: string;
  model_id: string | null;
  system_prompt: string | null;
  persona_id?: string | null;
  created_at: string;
  updated_at: string;
  messages: Message[];
  /** LLM-generated rolling summary of evicted conversation turns. */
  context_summary?: string | null;
  /** Rowid watermark: last message already absorbed into context_summary. */
  summary_up_to_rowid?: number | null;
}

export interface ConversationSummary {
  id: string;
  title: string;
  model_id: string | null;
  persona_id?: string | null;
  created_at: string;
  updated_at: string;
  message_count: number;
}

/** Per-image entry stored in message metadata. */
export interface ImageMeta {
  filename: string;
  mime: string;
  /** Base64-encoded image bytes — rendered as a data URL directly. */
  data: string;
}

export interface AttachmentMeta {
  attachments?: string[];
  /** Image attachments stored as base64 objects for persistent display. */
  images?: ImageMeta[];
}

/** Mirror of ToolStep used in the frontend skillsStore. */
export interface PersistedToolStep {
  tool_call_id: string;
  function_name: string;
  arguments: Record<string, unknown>;
  result?: string;
  turn: number;
}

export interface Message {
  id: string;
  conversation_id: string;
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  created_at: string;
  /** Metadata object, e.g. AttachmentMeta (serialized by Tauri as a JSON object) */
  metadata?: AttachmentMeta | null;
  /** Tool-call steps persisted with this assistant message. */
  tool_steps?: PersistedToolStep[] | null;
}

export interface HfModel {
  id: string;
  name: string;
  author: string;
  description: string | null;
  tags: string[];
  downloads: number | null;
  likes: number | null;
  last_modified: string | null;
  gguf_files: GgufFile[];
  is_downloaded: boolean;
  local_path: string | null;
}

export interface GgufFile {
  filename: string;
  size_bytes: number | null;
  quantization: string | null;
  url: string;
}

export interface DownloadProgress {
  model_id: string;
  filename: string;
  downloaded_bytes: number;
  total_bytes: number | null;
  status: "pending" | "downloading" | "completed" | "failed" | "cancelled";
}

export interface RagCollection {
  id: string;
  name: string;
  description: string | null;
  document_count: number;
  created_at: string;
  /** "hybrid" (BM25+cosine) or "graph" (GraphRAG sidecar). */
  retrieval_mode: "hybrid" | "graph";
  /** True once the graphrag-server sidecar has finished indexing this collection. */
  graph_indexed: boolean;
}

export interface MemoryEntry {
  id: string;
  content: string;
  source: string;
  created_at: string;
}

export interface AgentTask {
  id: string;
  title: string;
  description: string;
  status: "pending" | "running" | "completed" | "failed" | "cancelled";
  steps: AgentStep[];
  created_at: string;
  completed_at: string | null;
  result: string | null;
}

export interface AgentStep {
  step_number: number;
  thought: string;
  action: AgentAction | null;
  observation: string | null;
  created_at: string;
}

export interface AgentAction {
  tool_name: string;
  input: Record<string, unknown>;
  output: unknown | null;
  error: string | null;
}

export interface AgentEvent {
  task_id: string;
  event_type: string;
  payload: Record<string, unknown>;
}

export interface Flow {
  id: string;
  name: string;
  description: string | null;
  nodes: FlowNode[];
  edges: FlowEdge[];
  created_at: string;
  updated_at: string;
}

export interface FlowNode {
  id: string;
  node_type: string;
  position_x: number;
  position_y: number;
  data: Record<string, unknown>;
}

export interface FlowEdge {
  id: string;
  source: string;
  target: string;
  source_handle: string | null;
  target_handle: string | null;
}

export interface FlowExecution {
  id: string;
  flow_id: string;
  status: string;
  node_results: NodeResult[];
  started_at: string;
  completed_at: string | null;
}

export interface NodeResult {
  node_id: string;
  output: unknown;
  error: string | null;
  duration_ms: number;
}

export interface DbConnection {
  id: string;
  name: string;
  db_type: "mongodb" | "postgresql" | "mysql";
  connection_string: string;
  is_active: boolean;
  created_at: string;
}

export interface QueryResult {
  columns: string[];
  rows: Record<string, unknown>[];
  row_count: number;
  duration_ms: number;
}

export interface ServerStatus {
  running: boolean;
  port: number;
  model: string | null;
  binary_exists: boolean;
}

// ── Skills / MCP types ────────────────────────────────────────────────────

export type McpTransport =
  | { transport: "stdio"; command: string; args: string[] }
  | { transport: "http"; url: string; auth?: string };

export interface McpServerConfig {
  id: string;
  name: string;
  description: string;
  transport: McpTransport;
  builtin: boolean;
  enabled: boolean;
  icon: string;
}

export interface McpTool {
  name: string;
  description?: string;
  inputSchema: Record<string, unknown>;
}

export interface TaggedTool {
  server_id: string;
  server_name: string;
  tool: McpTool;
}

export interface ServerStatus {
  config: McpServerConfig;
  connected: boolean;
  tool_count: number;
}

export interface ToolCallEvent {
  conversation_id: string;
  tool_call_id: string;
  function_name: string;
  arguments: Record<string, unknown>;
  turn: number;
}

export interface ToolResultEvent {
  conversation_id: string;
  tool_call_id: string;
  function_name: string;
  result: string;
  turn: number;
}

export type ArtifactType = "code" | "markdown" | "html" | "text" | "csv" | "json" | "pdf";

export interface Artifact {
  id: string;
  conversation_id: string;
  message_id?: string;
  title: string;
  artifact_type: ArtifactType;
  language?: string;
  content: string;
  created_at: string;
  updated_at: string;
}

// ── Coding / AI coding agent types ───────────────────────────────────────────

export type CodingMode = "agent" | "plan" | "debug" | "ask";

export interface CodingSession {
  id: string;
  title: string;
  mode: CodingMode;
  project_path: string | null;
  created_at: string;
  updated_at: string;
}

export interface CodingEventPayload {
  event_type: string;
  payload: Record<string, unknown>;
}

export interface CodingMessage {
  id: string;
  session_id: string;
  role: "user" | "assistant";
  content: string;
  events: CodingEventPayload[];
  created_at: string;
}

export interface CodingPlanTask {
  id: string;
  title: string;
  description: string;
  status: "pending" | "in_progress" | "completed" | "failed";
  note: string | null;
}

export interface CodingPlan {
  id: string;
  session_id: string;
  title: string;
  tasks: CodingPlanTask[];
  status: "pending" | "in_progress" | "completed";
  created_at: string;
  updated_at: string;
}

export interface CodingEvent {
  session_id: string;
  event_type: string;
  payload: Record<string, unknown>;
}

export interface FileTreeEntry {
  name: string;
  path: string;
  type: "file" | "directory";
  size?: number;
  children?: FileTreeEntry[];
}

export interface AppSettings {
  models_directory: string;
  default_engine_mode: string;
  remote_server_url: string | null;
  remote_api_key: string | null;
  hf_api_token: string | null;
  embedding_model_path: string | null;
  theme: string;
  language: string;
  auto_sync_models: boolean;
  max_agent_iterations: number;
  agent_timeout_seconds: number;
  llama_server_port: number;
  n_gpu_layers: number;
  server_threads: number;
  server_context_size: number;
  server_batch_size: number;
  flash_attention: boolean;
  use_mmap: boolean;
  last_server_model: string | null;
  /** Minutes of inactivity before auto-stopping the server to free VRAM. 0 = never. */
  model_keep_alive_mins: number;
  reasoning_format: string;
  enable_thinking: boolean;
  /** Max tokens for the reasoning/thinking phase. 0 = unlimited (can cause no-response). */
  thinking_budget_tokens: number;
  /** Max tokens for the visible response (thinking budget is separate). */
  max_response_tokens: number;
  /** Allow the LLM to execute code in a sandboxed subprocess and see the output. */
  enable_code_execution: boolean;
  /** Path to the multimodal projection file (mmproj-*.gguf) for VLM models. */
  mmproj_path: string | null;
  /** Automatically extract and recall key facts from conversations. */
  memory_enabled: boolean;
  /** Enable the mobile HTTP/SSE bridge server. */
  mobile_api_enabled: boolean;
  /** Port for the mobile bridge server (default 3847). */
  mobile_api_port: number;
  /** Optional bearer token for mobile API authentication. */
  mobile_api_token: string | null;
  // ── Knowledge Base ────────────────────────────────────────────────────────
  /** fastembed model name for semantic embeddings (default: nomic-embed-text-v1.5). */
  embedding_model: string;
  /** Weight for cosine similarity in hybrid BM25+cosine search (0–1, default 0.6). */
  hybrid_cosine_weight: number;
  // ── GraphRAG sidecar ──────────────────────────────────────────────────────
  /** Enable the GraphRAG sidecar process. */
  graph_rag_enabled: boolean;
  /** Port for the graphrag-server sidecar (default 3848). */
  graph_rag_port: number;
  /** Override path to the graphrag-server binary. */
  graph_rag_server_path: string | null;
  /** Auto-start the sidecar when the app launches. */
  graph_rag_auto_start: boolean;
  /** Vector DB backend for graphrag-server ("lancedb" | "qdrant"). */
  graph_rag_vector_db: string;

  // User profile — collected during onboarding
  onboarding_completed: boolean;
  user_name?: string;
  user_profession?: string;
  user_about?: string;

  // Voice input (whisper.cpp)
  whisper_enabled: boolean;
  whisper_model_path?: string;
  /** BCP-47 language code, or "auto" for auto-detection. */
  whisper_language: string;
  /** Port the whisper-server sidecar listens on (default 8765). */
  whisper_port: number;
  /** Which whisper-server build to download: "cpu" or "cuda". */
  whisper_variant: string;

  // Voice output (KokoroTTS)
  tts_enabled: boolean;
  /** Port the kokoro_server.py sidecar listens on (default 8766). */
  tts_port: number;
  /** Selected voice ID, e.g. "af_heart". */
  tts_voice: string;
  /** Speech rate multiplier (1.0 = normal). */
  tts_speed: number;
  /** BCP-47 language code for TTS, e.g. "en-us", "pt-br". */
  tts_language: string;
  /** Torch device for KokoroTTS: "cpu", "cuda11", or "cuda12". */
  tts_device: string;
}

export interface GalleryImage {
  id: string;
  conversation_id: string;
  source: "generated" | "upload";
  filename: string;
  image_data: string;
  mime_type: string;
  prompt: string | null;
  width: number | null;
  height: number | null;
  created_at: string;
  /** Absolute path to the image file on disk (new images). */
  file_path?: string | null;
}
