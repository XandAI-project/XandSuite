import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export { invoke, listen };
export type { UnlistenFn };

// Type-safe wrappers for Tauri commands

export interface Conversation {
  id: string;
  title: string;
  model_id: string | null;
  system_prompt: string | null;
  created_at: string;
  updated_at: string;
  messages: Message[];
}

export interface ConversationSummary {
  id: string;
  title: string;
  model_id: string | null;
  created_at: string;
  updated_at: string;
  message_count: number;
}

export interface AttachmentMeta {
  attachments?: string[];
  /** Full filesystem paths for image attachments — used to load thumbnails. */
  images?: string[];
}

export interface Message {
  id: string;
  conversation_id: string;
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  created_at: string;
  /** Metadata object, e.g. AttachmentMeta (serialized by Tauri as a JSON object) */
  metadata?: AttachmentMeta | null;
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

export type ArtifactType = "code" | "markdown" | "html" | "text";

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
}
