import { api, uploadFile } from "./client";
import {
  AgentTask,
  AppSettings,
  Artifact,
  ComfyWorkflow,
  Conversation,
  DbConnection,
  GalleryImage,
  McpServerConfig,
  Message,
  RagCollection,
} from "../lib/types";

// ── Conversations ──────────────────────────────────────────────────────────────

export const chatApi = {
  listConversations: () => api.get<Conversation[]>("/conversations"),
  createConversation: (title?: string, system_prompt?: string) =>
    api.post<{ id: string }>("/conversations", { title, system_prompt }),
  getConversation: (id: string) => api.get<Conversation>(`/conversations/${id}`),
  updateConversation: (id: string, data: { title?: string; system_prompt?: string }) =>
    api.put<{ success: boolean }>(`/conversations/${id}`, data),
  deleteConversation: (id: string) =>
    api.delete<{ success: boolean }>(`/conversations/${id}`),
  truncateConversation: (id: string) =>
    api.post<{ success: boolean }>(`/conversations/${id}/truncate`),

  sendMessage: (params: {
    conversation_id: string;
    content: string;
    use_rag?: boolean;
    rag_collection_id?: string | null;
    use_skills?: boolean;
    attachments?: string[];
  }) => api.post<{ assistant_msg_id: string }>("/messages/send", params),

  saveToolSteps: (messageId: string, tool_steps_json: string) =>
    api.post<{ success: boolean }>(`/messages/${messageId}/tool-steps`, { tool_steps_json }),
};

// ── Artifacts ─────────────────────────────────────────────────────────────────

export const artifactApi = {
  list: (conversation_id?: string) =>
    api.get<Artifact[]>(`/artifacts${conversation_id ? `?conversation_id=${conversation_id}` : ""}`),
  listAll: () => api.get<Artifact[]>("/artifacts/all"),
  save: (data: Omit<Artifact, "id" | "created_at" | "updated_at">) =>
    api.post<{ id: string }>("/artifacts", data),
  update: (id: string, data: Partial<Pick<Artifact, "title" | "content" | "language">>) =>
    api.put<{ success: boolean }>(`/artifacts/${id}`, data),
  delete: (id: string) => api.delete<{ success: boolean }>(`/artifacts/${id}`),
};

// ── Gallery ───────────────────────────────────────────────────────────────────

export const galleryApi = {
  list: (conversation_id?: string) =>
    api.get<GalleryImage[]>(`/gallery${conversation_id ? `?conversation_id=${conversation_id}` : ""}`),
  listAll: () => api.get<GalleryImage[]>("/gallery/all"),
  delete: (id: string) => api.delete<{ success: boolean }>(`/gallery/${id}`),
  upload: (fileUri: string, filename: string, mimeType: string, conversation_id?: string) =>
    uploadFile("/gallery/upload", fileUri, filename, mimeType,
      conversation_id ? { conversation_id } : undefined),
};

// ── Settings ──────────────────────────────────────────────────────────────────

export const settingsApi = {
  get: () => api.get<AppSettings>("/settings"),
  save: (settings: AppSettings) => api.post<{ success: boolean }>("/settings", settings),
  getDataDir: () => api.get<{ data_dir: string }>("/settings/data-dir"),
};

// ── Server ────────────────────────────────────────────────────────────────────

export const serverApi = {
  getStatus: () => api.get<{ running: boolean; model: string | null; port: number }>("/server/status"),
  start: (model_path: string) => api.post<{ success: boolean }>("/server/start", { model_path }),
  stop: () => api.post<{ success: boolean }>("/server/stop"),
  detectGpu: () => api.get<{ name: string; recommended_variant: string; reason: string }>("/server/detect-gpu"),
};

// ── Models ────────────────────────────────────────────────────────────────────

export const modelApi = {
  listHf: () => api.get<unknown[]>("/models/hf"),
  listDownloaded: () => api.get<{ path: string; filename: string; size_bytes: number }[]>("/models/downloaded"),
  load: (model_path: string) => api.post<{ success: boolean }>("/models/load", { model_path }),
  connectRemote: (url: string, api_key?: string, model_id?: string) =>
    api.post<{ success: boolean }>("/models/remote", { url, api_key, model_id }),
  isEngineLoaded: () => api.get<{ loaded: boolean }>("/models/engine-loaded"),
  delete: (filename: string) => api.delete<{ success: boolean }>(`/models/${filename}`),
};

// ── RAG ───────────────────────────────────────────────────────────────────────

export const ragApi = {
  listCollections: () => api.get<RagCollection[]>("/rag"),
  createCollection: (name: string, description?: string) =>
    api.post<{ id: string; name: string }>("/rag", { name, description }),
  deleteCollection: (id: string) => api.delete<{ success: boolean }>(`/rag/${id}`),
  ingest: (collection_id: string, text: string, source?: string) =>
    api.post<{ success: boolean }>(`/rag/${collection_id}/ingest`, { text, source }),
  search: (query: string, collection_id?: string, limit = 10) =>
    api.post<unknown[]>("/rag/search", { query, collection_id, limit }),
  setRetrievalMode: (collection_id: string, mode: "hybrid" | "graph") =>
    api.put<{ success: boolean }>(`/rag/${collection_id}/mode`, { mode }),
};

// ── Skills ────────────────────────────────────────────────────────────────────

export const skillsApi = {
  listServers: () => api.get<unknown[]>("/skills/servers"),
  listTools: () => api.get<unknown[]>("/skills/tools"),
  addServer: (config: McpServerConfig) => api.post<{ success: boolean }>("/skills/servers", config),
  removeServer: (id: string) => api.delete<{ success: boolean }>(`/skills/servers/${id}`),
  callTool: (tool_name: string, arguments_: unknown, conv_id?: string) =>
    api.post<{ result: unknown }>("/skills/tools/call", { tool_name, arguments: arguments_, conv_id }),
  reloadBuiltins: () => api.post<{ started: boolean }>("/skills/reload-builtins"),
};

// ── Memory ────────────────────────────────────────────────────────────────────

export const memoryApi = {
  list: () => api.get<unknown[]>("/memory"),
  delete: (id: string) => api.delete<{ success: boolean }>(`/memory/${id}`),
  clear: () => api.delete<{ success: boolean }>("/memory"),
};

// ── Agents ────────────────────────────────────────────────────────────────────

export const agentApi = {
  list: () => api.get<AgentTask[]>("/agents"),
  run: (title: string, description: string) =>
    api.post<{ task_id: string }>("/agents", { title, description }),
  delete: (id: string) => api.delete<{ success: boolean }>(`/agents/${id}`),
  cancel: (id: string) => api.post<{ success: boolean }>(`/agents/${id}/cancel`),
  listFiles: (id: string) => api.get<{ name: string; path: string; size_bytes: number }[]>(`/agents/${id}/files`),
  readFile: (id: string, path: string) =>
    api.get<{ content: string }>(`/agents/${id}/files/${path}`),
};

// ── Database ──────────────────────────────────────────────────────────────────

export const databaseApi = {
  listConnections: () => api.get<DbConnection[]>("/database/connections"),
  addConnection: (name: string, db_type: string, connection_string: string) =>
    api.post<{ id: string }>("/database/connections", { name, db_type, connection_string }),
  deleteConnection: (id: string) => api.delete<{ success: boolean }>(`/database/connections/${id}`),
  testConnection: (id: string) =>
    api.post<{ success: boolean }>(`/database/connections/${id}/test`, {}),
  executeQuery: (connection_id: string, query: string) =>
    api.post<{ columns: string[]; rows: unknown[]; row_count: number; duration_ms: number }>(
      "/database/query",
      { connection_id, query }
    ),
  // Short-form aliases used by screens
  list: () => api.get<DbConnection[]>("/database/connections"),
  add: (name: string, connection_string: string, db_type: string) =>
    api.post<{ id: string }>("/database/connections", { name, db_type, connection_string }),
  delete: (id: string) => api.delete<{ success: boolean }>(`/database/connections/${id}`),
  test: (id: string) =>
    api.post<{ success: boolean }>(`/database/connections/${id}/test`, {}).then((r) => r.success),
  query: (connection_id: string, query: string) =>
    api.post<unknown>("/database/query", { connection_id, query }),
};

// ── ComfyUI ───────────────────────────────────────────────────────────────────

export const comfyApi = {
  listWorkflows: () => api.get<ComfyWorkflow[]>("/comfyui/workflows"),
  saveWorkflow: (name: string, workflow: Record<string, unknown>) =>
    api.post<{ id: string }>("/comfyui/workflows", { name, workflow }),
  deleteWorkflow: (id: string) => api.delete<{ success: boolean }>(`/comfyui/workflows/${id}`),
};

// ── Logs ──────────────────────────────────────────────────────────────────────

export const logsApi = {
  get: () => api.get<{ level: string; message: string; ts: string }[]>("/logs"),
};
