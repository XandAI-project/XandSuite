import { create } from "zustand";
import { memoryApi } from "../api/endpoints";

interface MemoryState {
  entries: unknown[];
  fetchEntries: () => Promise<void>;
  deleteEntry: (id: string) => Promise<void>;
  clearAll: () => Promise<void>;
}

export const useMemoryStore = create<MemoryState>((set, get) => ({
  entries: [],

  fetchEntries: async () => {
    const entries = await memoryApi.list();
    set({ entries });
  },

  deleteEntry: async (id) => {
    await memoryApi.delete(id);
    await get().fetchEntries();
  },

  clearAll: async () => {
    await memoryApi.clear();
    set({ entries: [] });
  },
}));
