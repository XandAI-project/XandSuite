import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { MemoryEntry } from "../lib/tauri";

interface MemoryState {
  entries: MemoryEntry[];
  isLoading: boolean;
  fetchEntries: () => Promise<void>;
  deleteEntry: (id: string) => Promise<void>;
  clearAll: () => Promise<void>;
}

export const useMemoryStore = create<MemoryState>((set) => ({
  entries: [],
  isLoading: false,

  fetchEntries: async () => {
    set({ isLoading: true });
    try {
      const entries = await invoke<MemoryEntry[]>("list_memory_entries");
      set({ entries });
    } catch (err) {
      console.error("Failed to fetch memory entries:", err);
    } finally {
      set({ isLoading: false });
    }
  },

  deleteEntry: async (id: string) => {
    await invoke("delete_memory_entry", { entryId: id });
    set((state) => ({
      entries: state.entries.filter((e) => e.id !== id),
    }));
  },

  clearAll: async () => {
    await invoke("clear_memory_entries");
    set({ entries: [] });
  },
}));
