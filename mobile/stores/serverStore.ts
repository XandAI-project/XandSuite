import { create } from "zustand";
import { serverApi } from "../api/endpoints";

interface ServerState {
  running: boolean;
  model: string | null;
  port: number;
  isLoading: boolean;
  error: string | null;

  fetchStatus: () => Promise<void>;
  startServer: (model_path: string) => Promise<void>;
  stopServer: () => Promise<void>;
}

export const useServerStore = create<ServerState>((set) => ({
  running: false,
  model: null,
  port: 11434,
  isLoading: false,
  error: null,

  fetchStatus: async () => {
    try {
      const status = await serverApi.getStatus();
      set({ running: status.running, model: status.model, port: status.port, error: null });
    } catch (e: unknown) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },

  startServer: async (model_path) => {
    set({ isLoading: true, error: null });
    try {
      await serverApi.start(model_path);
      await useServerStore.getState().fetchStatus();
    } catch (e: unknown) {
      set({ error: e instanceof Error ? e.message : String(e) });
    } finally {
      set({ isLoading: false });
    }
  },

  stopServer: async () => {
    set({ isLoading: true });
    try {
      await serverApi.stop();
      set({ running: false, model: null });
    } finally {
      set({ isLoading: false });
    }
  },
}));
