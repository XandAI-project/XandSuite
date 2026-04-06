import { create } from "zustand";
import { modelApi } from "../api/endpoints";
import { sseManager } from "../api/sse";

interface DownloadState {
  filename: string;
  downloaded_bytes: number;
  total_bytes: number | null;
  status: string;
}

interface ModelState {
  hfModels: unknown[];
  downloadedModels: { path: string; filename: string; size_bytes: number }[];
  engineLoaded: boolean;
  isLoading: boolean;
  downloads: Record<string, DownloadState>;

  fetchHfModels: () => Promise<void>;
  fetchDownloaded: () => Promise<void>;
  checkEngine: () => Promise<void>;
  loadModel: (path: string) => Promise<void>;
  connectRemote: (url: string, api_key?: string) => Promise<void>;
  deleteModel: (filename: string) => Promise<void>;
}

export const useModelStore = create<ModelState>((set, get) => {
  sseManager.on("download_progress", (event) => {
    if (event.type !== "download_progress") return;
    set((s) => ({
      downloads: {
        ...s.downloads,
        [event.model_id]: {
          filename: event.filename,
          downloaded_bytes: event.downloaded_bytes,
          total_bytes: event.total_bytes,
          status: event.status,
        },
      },
    }));
    if (event.status === "completed") {
      setTimeout(() => get().fetchDownloaded(), 500);
    }
  });

  return {
    hfModels: [],
    downloadedModels: [],
    engineLoaded: false,
    isLoading: false,
    downloads: {},

    fetchHfModels: async () => {
      set({ isLoading: true });
      try {
        const models = await modelApi.listHf();
        set({ hfModels: models });
      } finally {
        set({ isLoading: false });
      }
    },

    fetchDownloaded: async () => {
      const models = await modelApi.listDownloaded();
      set({ downloadedModels: models });
    },

    checkEngine: async () => {
      const r = await modelApi.isEngineLoaded();
      set({ engineLoaded: r.loaded });
    },

    loadModel: async (path) => {
      set({ isLoading: true });
      try {
        await modelApi.load(path);
        await get().checkEngine();
      } finally {
        set({ isLoading: false });
      }
    },

    connectRemote: async (url, api_key) => {
      await modelApi.connectRemote(url, api_key);
      await get().checkEngine();
    },

    deleteModel: async (filename) => {
      await modelApi.delete(filename);
      await get().fetchDownloaded();
    },
  };
});
