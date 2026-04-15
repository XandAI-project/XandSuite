import { create } from "zustand";
import { invoke } from "@/lib/tauri";
import { listen } from "@tauri-apps/api/event";
import type { AppSettings, DownloadProgress } from "@/lib/tauri";

export interface ServerStatus {
  running: boolean;
  port: number;
  model: string | null;
  binary_exists: boolean;
}

export interface GpuInfo {
  name: string;
  recommended_variant: string;
  reason: string;
}

interface ServerStore {
  status: ServerStatus;
  gpuInfo: GpuInfo | null;
  isStarting: boolean;
  isStopping: boolean;
  isDownloading: boolean;
  downloadProgress: DownloadProgress | null;
  error: string | null;
  /** "local" or "remote" — mirrors AppSettings.default_engine_mode */
  engineMode: string;
  /** Last model path used with the local server — mirrors AppSettings.last_server_model */
  lastModel: string | null;

  fetchStatus: () => Promise<void>;
  detectGpu: () => Promise<void>;
  startServer: (modelPath: string, mmprojPath?: string) => Promise<void>;
  stopServer: () => Promise<void>;
  downloadBinary: (variant: "cpu" | "cuda12" | "cuda13" | "vulkan") => Promise<void>;
  listenToProgress: () => Promise<() => void>;
}

const defaultStatus: ServerStatus = {
  running: false,
  port: 11434,
  model: null,
  binary_exists: false,
};

export const useServerStore = create<ServerStore>((set, get) => ({
  status: defaultStatus,
  gpuInfo: null,
  isStarting: false,
  isStopping: false,
  isDownloading: false,
  downloadProgress: null,
  error: null,
  engineMode: "local",
  lastModel: null,

  detectGpu: async () => {
    try {
      const info = await invoke<GpuInfo>("detect_gpu");
      set({ gpuInfo: info });
    } catch (e) {
      console.warn("GPU detection failed:", e);
    }
  },

  fetchStatus: async () => {
    try {
      const [status, settings] = await Promise.all([
        invoke<ServerStatus>("get_server_status"),
        invoke<AppSettings>("get_settings"),
      ]);
      set({
        status,
        engineMode: settings.default_engine_mode ?? "local",
        lastModel: settings.last_server_model ?? null,
      });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  startServer: async (modelPath: string, mmprojPath?: string) => {
    set({ isStarting: true, error: null });
    try {
      await invoke("start_local_server", {
        modelPath,
        mmprojPath: mmprojPath ?? null,
      });
      set({ error: null });
      await get().fetchStatus();
    } catch (e) {
      // Tauri wraps the error string — unwrap it if possible
      const msg = e instanceof Error ? e.message : String(e);
      set({ error: msg });
    } finally {
      set({ isStarting: false });
    }
  },

  stopServer: async () => {
    set({ isStopping: true, error: null });
    try {
      await invoke("stop_local_server");
      await get().fetchStatus();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ isStopping: false });
    }
  },

  downloadBinary: async (variant: "cpu" | "cuda12" | "cuda13" | "vulkan") => {
    set({ isDownloading: true, downloadProgress: null, error: null });
    try {
      await invoke("download_llama_server", { variant });
      await get().fetchStatus();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ isDownloading: false });
    }
  },

  listenToProgress: async () => {
    const unlisten = await listen<DownloadProgress>("server_binary_progress", (event) => {
      set({ downloadProgress: event.payload });
      if (event.payload.status === "completed") {
        // Refresh binary_exists flag
        get().fetchStatus();
      }
    });
    return unlisten;
  },
}));
