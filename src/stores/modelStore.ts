import { create } from "zustand";
import { invoke } from "../lib/tauri";
import { listen } from "@tauri-apps/api/event";
import type { DownloadProgress, HfModel } from "../lib/tauri";

interface DownloadState {
  [key: string]: DownloadProgress;
}

/** Outcome of a remote-server connection attempt. `error` holds the backend's
 *  reason (unreachable host, non-LLM endpoint, …) when there is one. */
export interface ConnectRemoteResult {
  ok: boolean;
  error?: string;
}

interface ModelStore {
  models: HfModel[];
  downloadedModels: { model_id: string; filename: string; path: string; size_bytes: number }[];
  downloads: DownloadState;
  isEngineLoaded: boolean;
  isLoading: boolean;
  error: string | null;

  fetchModels: (search?: string) => Promise<void>;
  refreshModels: () => Promise<void>;
  fetchDownloadedModels: () => Promise<void>;
  downloadModel: (modelId: string, filename: string, url: string) => Promise<void>;
  deleteModel: (modelId: string, filename: string) => Promise<void>;
  loadModel: (modelPath: string) => Promise<void>;
  connectRemote: (url: string, apiKey?: string, modelName?: string) => Promise<ConnectRemoteResult>;
  checkEngineStatus: () => Promise<void>;
  listenToDownloads: () => Promise<() => void>;
}

export const useModelStore = create<ModelStore>((set, get) => ({
  models: [],
  downloadedModels: [],
  downloads: {},
  isEngineLoaded: false,
  isLoading: false,
  error: null,

  fetchModels: async (search?: string) => {
    set({ isLoading: true, error: null });
    try {
      const models = await invoke<HfModel[]>("list_hf_models", {
        search: search || null,
        limit: 50,
      });
      set({ models, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  refreshModels: async () => {
    set({ isLoading: true });
    try {
      await invoke("refresh_hf_models");
      await get().fetchModels();
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  fetchDownloadedModels: async () => {
    try {
      const models = await invoke<{ model_id: string; filename: string; path: string; size_bytes: number }[]>(
        "list_downloaded_models"
      );
      set({ downloadedModels: models });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  downloadModel: async (modelId: string, filename: string, url: string) => {
    try {
      await invoke("download_model", { modelId, filename, url });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  deleteModel: async (modelId: string, filename: string) => {
    try {
      await invoke("delete_model", { modelId, filename });
      await get().fetchDownloadedModels();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  loadModel: async (modelPath: string) => {
    try {
      await invoke("load_model", { modelPath });
      set({ isEngineLoaded: true });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  connectRemote: async (url: string, apiKey?: string, modelName?: string) => {
    try {
      // Tauri returns a bare boolean; the HTTP route returns
      // { success, reachable, error }.
      const res = await invoke<boolean | { reachable?: boolean; success?: boolean; error?: string | null }>(
        "connect_remote_server",
        {
          serverUrl: url,
          apiKey: apiKey || null,
          modelName: modelName || null,
        }
      );
      const ok = typeof res === "boolean" ? res : Boolean(res?.reachable ?? res?.success);
      const error = typeof res === "boolean" ? undefined : res?.error ?? undefined;
      set({ isEngineLoaded: ok, error: error ?? null });
      return { ok, error };
    } catch (e) {
      const error = String(e);
      set({ error, isEngineLoaded: false });
      return { ok: false, error };
    }
  },

  checkEngineStatus: async () => {
    try {
      const loaded = await invoke<boolean>("is_engine_loaded");
      set({ isEngineLoaded: loaded });
    } catch {
      set({ isEngineLoaded: false });
    }
  },

  listenToDownloads: async () => {
    const unlisten = await listen<DownloadProgress>("download_progress", (event) => {
      const p = event.payload;
      const key = `${p.model_id}::${p.filename}`;
      set((state) => ({
        downloads: { ...state.downloads, [key]: p },
      }));
      if (p.status === "completed") {
        get().fetchDownloadedModels();
      }
    });
    return unlisten;
  },
}));
