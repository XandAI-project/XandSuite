// Shared types mirroring the Rust backend models and desktop src/lib/tauri.ts

export interface Conversation {
  id: string;
  title: string;
  model_id: string | null;
  system_prompt: string | null;
  created_at: string;
  updated_at: string;
  messages: Message[];
}

export interface Message {
  id: string;
  conversation_id: string;
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  created_at: string;
  token_count?: number | null;
  metadata?: MessageMetadata | null;
  tool_steps?: PersistedToolStep[] | null;
  // Convenience fields populated client-side from metadata or tool_steps
  tool_calls?: ToolCall[];
  tool_results?: Record<string, string>;
  images?: string[];
}

export interface ToolCall {
  id: string;
  type?: string;
  function?: {
    name: string;
    arguments: unknown;
  };
}

export interface RagSource {
  content: string;
  source: string;
  score: number;
  entities?: string[];
}

export interface MessageMetadata {
  attachments?: string[];
  images?: ImageMeta[];
  sources?: RagSource[];
}

export interface ImageMeta {
  filename: string;
  mime: string;
  data: string; // base64
}

export interface PersistedToolStep {
  id: string;
  tool_call_id: string;
  function_name: string;
  arguments: Record<string, unknown>;
  result?: string | null;
  status: "pending" | "running" | "done" | "error";
  turn: number;
  gallery_id?: string | null;
  image_url?: string | null;
}

export interface Artifact {
  id: string;
  conversation_id: string;
  message_id: string | null;
  title: string;
  artifact_type: "code" | "html" | "markdown" | "text" | "csv" | "json";
  language: string | null;
  content: string;
  created_at: string;
  updated_at: string;
}

export interface GalleryImage {
  id: string;
  conversation_id: string | null;
  source: "generated" | "upload";
  filename: string;
  mime_type: string;
  data: string; // base64
  data_url?: string | null; // prebuilt data: URI if provided by backend
  created_at: string;
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
  model_keep_alive_mins: number;
  reasoning_format: string;
  enable_thinking: boolean;
  thinking_budget_tokens: number;
  max_response_tokens: number;
  enable_code_execution: boolean;
  mmproj_path: string | null;
  memory_enabled: boolean;
  comfyui_url: string | null;
  comfyui_model: string | null;
  comfyui_model_type: string | null;
  comfyui_clip_name: string | null;
  comfyui_vae_name: string | null;
  mobile_api_enabled: boolean;
  mobile_api_port: number;
  mobile_api_token: string | null;
  embedding_model: string;
  hybrid_cosine_weight: number;
  graph_rag_enabled: boolean;
  graph_rag_port: number;
  graph_rag_server_path: string | null;
  graph_rag_auto_start: boolean;
  graph_rag_vector_db: string;
}

export interface RagCollection {
  id: string;
  name: string;
  description: string | null;
  document_count: number;
  created_at: string;
  retrieval_mode: "hybrid" | "graph";
  graph_indexed: boolean;
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
  input: unknown;
  output: unknown | null;
  error: string | null;
}

export interface McpServerConfig {
  id: string;
  name: string;
  transport: "stdio" | "http";
  command?: string;
  args?: string[];
  url?: string;
  auth?: string | null;
}

export interface McpTool {
  name: string;
  description: string;
  server_id: string;
  input_schema: Record<string, unknown>;
}

export interface DbConnection {
  id: string;
  name: string;
  db_type: "mongodb" | "postgresql" | "mysql";
  connection_string: string;
  is_active: boolean;
  created_at: string;
}

export interface ComfyWorkflow {
  id: string;
  name: string;
  workflow: Record<string, unknown>;
}

export interface LogEntry {
  level: "info" | "warn" | "warning" | "error" | "debug";
  message: string;
  ts: string;
}

// SSE event types (from api_server/events.rs)
export type ApiEvent =
  | { type: "chat_token"; conversation_id: string; token: string; done: boolean }
  | { type: "chat_thinking"; conversation_id: string; token: string }
  | { type: "chat_tool_call"; conversation_id: string; tool_call_id: string; function_name: string; arguments: Record<string, unknown>; turn: number }
  | { type: "chat_tool_result"; conversation_id: string; tool_call_id: string; result: string }
  | { type: "artifact_updated"; conversation_id: string; artifact_id: string }
  | { type: "gallery_updated"; conversation_id: string }
  | { type: "app_log"; level: string; message: string; ts: string }
  | { type: "download_progress"; model_id: string; filename: string; downloaded_bytes: number; total_bytes: number | null; status: string }
  | { type: "agent_event"; task_id: string; event_type: string; payload: unknown };
