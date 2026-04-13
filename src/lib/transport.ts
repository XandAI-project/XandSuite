/**
 * Unified transport layer for all backend communication.
 *
 * - Inside Tauri WebView  → native invoke() / listen()
 * - In a browser (web mode) → HTTP fetch() + SSE EventSource
 *
 * All stores and components import `invoke` and `listen` from here
 * (via @/lib/tauri which re-exports them), so nothing else needs to change.
 */

import {
  invoke as tauriInvoke,
  type InvokeArgs,
} from "@tauri-apps/api/core";
import {
  listen as tauriListen,
  type UnlistenFn,
  type Event as TauriEvent,
} from "@tauri-apps/api/event";
import { getServerUrl, getServerToken } from "./serverConfig";

// ── Runtime detection ─────────────────────────────────────────────────────────

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// ── Command → HTTP route map ──────────────────────────────────────────────────

interface RouteEntry {
  method: "GET" | "POST" | "PUT" | "DELETE";
  path: string;
  /**
   * Maps URL path params (`:name`) to the arg field that provides the value.
   * Example: { id: "conversation_id" } means :id ← args.conversation_id
   */
  pathParams?: Record<string, string>;
}

const CMD_MAP: Record<string, RouteEntry> = {
  // ── Chat ───────────────────────────────────────────────────────────────────
  list_conversations:        { method: "GET",    path: "/api/conversations" },
  create_conversation:       { method: "POST",   path: "/api/conversations" },
  get_conversation:          { method: "GET",    path: "/api/conversations/:id", pathParams: { id: "conversation_id" } },
  update_conversation:       { method: "PUT",    path: "/api/conversations/:id", pathParams: { id: "id" } },
  rename_conversation:       { method: "PUT",    path: "/api/conversations/:id", pathParams: { id: "id" } },
  delete_conversation:       { method: "DELETE", path: "/api/conversations/:id", pathParams: { id: "conversation_id" } },
  truncate_conversation:     { method: "POST",   path: "/api/conversations/:id/truncate", pathParams: { id: "conversation_id" } },
  send_message:              { method: "POST",   path: "/api/messages/send" },
  save_message_tool_steps:   { method: "POST",   path: "/api/messages/:id/tool-steps", pathParams: { id: "message_id" } },
  stop_generation:           { method: "POST",   path: "/api/chat/stop" },

  // ── Models ─────────────────────────────────────────────────────────────────
  list_hf_models:            { method: "GET",    path: "/api/models/hf" },
  refresh_hf_models:         { method: "POST",   path: "/api/models/hf/refresh" },
  list_downloaded_models:    { method: "GET",    path: "/api/models/downloaded" },
  download_model:            { method: "POST",   path: "/api/models/hf" },
  delete_model:              { method: "DELETE", path: "/api/models/:id", pathParams: { id: "model_id" } },
  load_model:                { method: "POST",   path: "/api/models/load" },
  connect_remote_server:     { method: "POST",   path: "/api/models/remote" },
  is_engine_loaded:          { method: "GET",    path: "/api/models/engine-loaded" },
  get_models_dir:            { method: "GET",    path: "/api/models/dir" },

  // ── Server ─────────────────────────────────────────────────────────────────
  get_server_status:         { method: "GET",    path: "/api/server/status" },
  start_local_server:        { method: "POST",   path: "/api/server/start" },
  stop_local_server:         { method: "POST",   path: "/api/server/stop" },
  download_llama_server:     { method: "POST",   path: "/api/server/download" },
  detect_gpu:                { method: "GET",    path: "/api/server/detect-gpu" },

  // ── Settings ───────────────────────────────────────────────────────────────
  get_settings:              { method: "GET",    path: "/api/settings" },
  save_settings:             { method: "POST",   path: "/api/settings" },
  get_data_dir:              { method: "GET",    path: "/api/settings/data-dir" },

  // ── RAG ────────────────────────────────────────────────────────────────────
  list_rag_collections:      { method: "GET",    path: "/api/rag" },
  create_rag_collection:     { method: "POST",   path: "/api/rag" },
  delete_rag_collection:     { method: "DELETE", path: "/api/rag/:id", pathParams: { id: "collection_id" } },
  ingest_document:           { method: "POST",   path: "/api/rag/:id/ingest", pathParams: { id: "collection_id" } },
  search_rag:                { method: "POST",   path: "/api/rag/search" },
  set_collection_retrieval_mode: { method: "PUT", path: "/api/rag/:id/mode", pathParams: { id: "collection_id" } },
  reindex_collection:        { method: "POST",   path: "/api/rag/:id/reindex", pathParams: { id: "collection_id" } },

  // ── Skills ─────────────────────────────────────────────────────────────────
  list_skill_servers:        { method: "GET",    path: "/api/skills/servers" },
  add_mcp_server:            { method: "POST",   path: "/api/skills/servers" },
  remove_mcp_server:         { method: "DELETE", path: "/api/skills/servers/:id", pathParams: { id: "server_id" } },
  list_tools:                { method: "GET",    path: "/api/skills/tools" },
  call_tool_direct:          { method: "POST",   path: "/api/skills/tools/call" },
  reload_builtin_servers:    { method: "POST",   path: "/api/skills/reload-builtins" },

  // ── Memory ─────────────────────────────────────────────────────────────────
  list_memory_entries:       { method: "GET",    path: "/api/memory" },
  delete_memory_entry:       { method: "DELETE", path: "/api/memory/:id", pathParams: { id: "entry_id" } },
  clear_memory_entries:      { method: "DELETE", path: "/api/memory" },

  // ── Agents ─────────────────────────────────────────────────────────────────
  run_agent_task:            { method: "POST",   path: "/api/agents" },
  list_agent_tasks:          { method: "GET",    path: "/api/agents" },
  delete_agent_task:         { method: "DELETE", path: "/api/agents/:id", pathParams: { id: "task_id" } },
  cancel_agent_task:         { method: "POST",   path: "/api/agents/:id/cancel", pathParams: { id: "task_id" } },
  list_task_files:           { method: "GET",    path: "/api/agents/:id/files", pathParams: { id: "task_id" } },
  open_task_workspace:       { method: "POST",   path: "/api/agents/:id/open-workspace", pathParams: { id: "task_id" } },

  // ── Flows ──────────────────────────────────────────────────────────────────
  list_flows:                { method: "GET",    path: "/api/flows" },
  save_flow:                 { method: "POST",   path: "/api/flows" },
  delete_flow:               { method: "DELETE", path: "/api/flows/:id", pathParams: { id: "flow_id" } },
  execute_flow:              { method: "POST",   path: "/api/flows/:id/execute", pathParams: { id: "flow_id" } },

  // ── Artifacts ──────────────────────────────────────────────────────────────
  save_artifact:             { method: "POST",   path: "/api/artifacts" },
  list_artifacts:            { method: "GET",    path: "/api/artifacts" },
  list_all_artifacts:        { method: "GET",    path: "/api/artifacts/all" },
  update_artifact:           { method: "PUT",    path: "/api/artifacts/:id", pathParams: { id: "id" } },
  delete_artifact:           { method: "DELETE", path: "/api/artifacts/:id", pathParams: { id: "id" } },

  // ── Gallery ────────────────────────────────────────────────────────────────
  list_gallery_images:       { method: "GET",    path: "/api/gallery" },
  list_all_gallery_images:   { method: "GET",    path: "/api/gallery/all" },
  delete_gallery_image:      { method: "DELETE", path: "/api/gallery/:id", pathParams: { id: "image_id" } },
  save_upload_to_gallery:    { method: "POST",   path: "/api/gallery/upload" },

  // ── Database ───────────────────────────────────────────────────────────────
  list_db_connections:       { method: "GET",    path: "/api/database/connections" },
  add_db_connection:         { method: "POST",   path: "/api/database/connections" },
  delete_db_connection:      { method: "DELETE", path: "/api/database/connections/:id", pathParams: { id: "connection_id" } },
  test_db_connection:        { method: "POST",   path: "/api/database/test" },
  execute_db_query:          { method: "POST",   path: "/api/database/query" },

  // ── Personas ───────────────────────────────────────────────────────────────
  list_personas:             { method: "GET",    path: "/api/personas" },
  get_persona:               { method: "GET",    path: "/api/personas/:id", pathParams: { id: "persona_id" } },
  create_persona:            { method: "POST",   path: "/api/personas" },
  update_persona:            { method: "PUT",    path: "/api/personas/:id", pathParams: { id: "id" } },
  delete_persona:            { method: "DELETE", path: "/api/personas/:id", pathParams: { id: "persona_id" } },

  // ── Templates ──────────────────────────────────────────────────────────────
  list_templates:            { method: "GET",    path: "/api/templates" },
  create_template:           { method: "POST",   path: "/api/templates" },
  update_template:           { method: "PUT",    path: "/api/templates/:id", pathParams: { id: "id" } },
  delete_template:           { method: "DELETE", path: "/api/templates/:id", pathParams: { id: "template_id" } },
  increment_template_use:    { method: "POST",   path: "/api/templates/:id/use", pathParams: { id: "template_id" } },

  // ── Packages ───────────────────────────────────────────────────────────────
  list_official_packages:    { method: "GET",    path: "/api/packages/official" },
  install_package:           { method: "POST",   path: "/api/packages/official/:id/install", pathParams: { id: "package_id" } },
  uninstall_package:         { method: "DELETE", path: "/api/packages/official/:id", pathParams: { id: "package_id" } },
  list_custom_packages:      { method: "GET",    path: "/api/packages/custom" },
  save_custom_package:       { method: "POST",   path: "/api/packages/custom" },
  get_custom_package_code:   { method: "GET",    path: "/api/packages/custom/:id/code", pathParams: { id: "id" } },
  install_custom_package:    { method: "POST",   path: "/api/packages/custom/:id/install", pathParams: { id: "id" } },
  uninstall_custom_package:  { method: "POST",   path: "/api/packages/custom/:id/uninstall", pathParams: { id: "id" } },
  delete_custom_package:     { method: "DELETE", path: "/api/packages/custom/:id", pathParams: { id: "id" } },
  fetch_comfyui_workflows:   { method: "POST",   path: "/api/comfyui/fetch" },

  // ── Whisper ────────────────────────────────────────────────────────────────
  get_whisper_status:        { method: "GET",    path: "/api/whisper/status" },
  start_whisper_server:      { method: "POST",   path: "/api/whisper/start" },
  stop_whisper_server:       { method: "POST",   path: "/api/whisper/stop" },
  transcribe_audio:          { method: "POST",   path: "/api/whisper/transcribe" },
  download_whisper_binary:   { method: "POST",   path: "/api/whisper/download-binary" },
  download_whisper_model:    { method: "POST",   path: "/api/whisper/download-model" },

  // ── TTS (KokoroTTS) ────────────────────────────────────────────────────────
  get_tts_status:            { method: "GET",    path: "/api/tts/status" },
  start_tts_server:          { method: "POST",   path: "/api/tts/start" },
  stop_tts_server:           { method: "POST",   path: "/api/tts/stop" },
  synthesize_speech:         { method: "POST",   path: "/api/tts/synthesize" },
  download_tts_models:       { method: "POST",   path: "/api/tts/download-models" },
  setup_tts_deps:            { method: "POST",   path: "/api/tts/setup-deps" },
  get_tts_log:               { method: "GET",    path: "/api/tts/log" },

  // ── Logs ───────────────────────────────────────────────────────────────────
  // (logs come via SSE events, not invoke)

  // ── ComfyUI workflows ──────────────────────────────────────────────────────
  // (CRUD goes through skills/comfyui routes already in the handler)

  // ── File access ─────────────────────────────────────────────────────────────
  read_file_as_base64:       { method: "POST",   path: "/api/files/base64" },
};

// ── HTTP invoke ───────────────────────────────────────────────────────────────

async function httpInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const route = CMD_MAP[cmd];
  if (!route) {
    throw new Error(`[transport] No HTTP route mapping for command: "${cmd}"`);
  }

  // Substitute path params; remaining args go in the body / query string
  let path = route.path;
  const remaining: Record<string, unknown> = args ? { ...args } : {};

  for (const [urlParam, argField] of Object.entries(route.pathParams ?? {})) {
    const value =
      (remaining[argField] as string | undefined) ??
      (remaining[urlParam] as string | undefined);
    if (value !== undefined) {
      path = path.replace(`:${urlParam}`, encodeURIComponent(String(value)));
      delete remaining[argField];
      delete remaining[urlParam];
    }
  }

  const base = getServerUrl();
  const url = `${base}${path}`;

  const headers: Record<string, string> = { "Content-Type": "application/json" };
  const token = getServerToken();
  if (token) headers["Authorization"] = `Bearer ${token}`;

  const init: RequestInit = { method: route.method, headers };

  if (route.method !== "GET" && route.method !== "DELETE") {
    if (Object.keys(remaining).length > 0) {
      init.body = JSON.stringify(remaining);
    }
  }

  const res = await fetch(url, init);

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(text || `HTTP ${res.status} — ${url}`);
  }

  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

// ── Public invoke ─────────────────────────────────────────────────────────────

export async function invoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  if (isTauri()) {
    return tauriInvoke<T>(cmd, args);
  }
  return httpInvoke<T>(cmd, args as Record<string, unknown> | undefined);
}

// ── SSE event bus (web mode) ──────────────────────────────────────────────────

let _sseSource: EventSource | null = null;
let _sseCounter = 0;
const _sseListeners = new Map<
  string,
  Set<(event: TauriEvent<unknown>) => void>
>();

function getSseSource(): EventSource {
  if (_sseSource && _sseSource.readyState !== EventSource.CLOSED) {
    return _sseSource;
  }

  const base = getServerUrl();
  const token = getServerToken();
  const url = token
    ? `${base}/api/events?token=${encodeURIComponent(token)}`
    : `${base}/api/events`;

  _sseSource = new EventSource(url);

  _sseSource.onmessage = (evt) => {
    try {
      const data = JSON.parse(evt.data) as Record<string, unknown>;
      // Server wraps events as: { event: "chat_token", payload: ... }
      const eventName = (data["event"] ?? data["type"]) as string | undefined;
      const payload = data["payload"] ?? data;
      if (eventName) {
        const tauriEvent: TauriEvent<unknown> = {
          event: eventName,
          id: ++_sseCounter,
          payload,
        };
        _sseListeners.get(eventName)?.forEach((cb) => cb(tauriEvent));
      }
    } catch {
      // ignore malformed frames
    }
  };

  _sseSource.onerror = () => {
    // EventSource reconnects automatically
  };

  return _sseSource;
}

// ── Public listen ─────────────────────────────────────────────────────────────
// Signature matches @tauri-apps/api/event exactly so all existing stores
// that use `event.payload` continue to work unchanged.

export async function listen<T>(
  event: string,
  handler: (event: TauriEvent<T>) => void
): Promise<UnlistenFn> {
  if (isTauri()) {
    return tauriListen<T>(event, handler);
  }

  // Web mode — subscribe to the SSE stream
  getSseSource();

  if (!_sseListeners.has(event)) {
    _sseListeners.set(event, new Set());
  }
  const cb = handler as (event: TauriEvent<unknown>) => void;
  _sseListeners.get(event)!.add(cb);

  return () => {
    _sseListeners.get(event)?.delete(cb);
  };
}

export type { UnlistenFn };
