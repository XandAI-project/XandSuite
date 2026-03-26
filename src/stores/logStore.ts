import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";

const uid = () => `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;

export type LogLevel = "info" | "warn" | "error" | "debug";

export interface LogEntry {
  id: string;
  level: LogLevel;
  message: string;
  ts: string;
}

interface LogPayload {
  level: string;
  message: string;
  ts: string;
}

const MAX_ENTRIES = 500;

interface LogStore {
  entries: LogEntry[];
  unlistenFn: (() => void) | null;
  add: (entry: Omit<LogEntry, "id">) => void;
  clear: () => void;
  init: () => Promise<void>;
  destroy: () => void;
}

export const useLogStore = create<LogStore>((set, get) => ({
  entries: [],
  unlistenFn: null,

  add: (entry) =>
    set((state) => {
      const newEntries = [{ ...entry, id: uid() }, ...state.entries];
      return { entries: newEntries.slice(0, MAX_ENTRIES) };
    }),

  clear: () => set({ entries: [] }),

  init: async () => {
    if (get().unlistenFn) return; // already listening

    // Set a sentinel immediately (synchronous, before the await) so that any
    // concurrent call (e.g. React StrictMode double-invoke) hits the guard
    // above and returns early rather than registering a second listener.
    set({ unlistenFn: () => {} });

    const unlisten = await listen<LogPayload>("app_log", (event) => {
      const { level, message, ts } = event.payload;
      get().add({
        level: (["info", "warn", "error", "debug"].includes(level)
          ? level
          : "info") as LogLevel,
        message,
        ts,
      });
    });

    // Replace sentinel with the real unlisten handle
    set({ unlistenFn: unlisten });
  },

  destroy: () => {
    const { unlistenFn } = get();
    if (unlistenFn) {
      unlistenFn();
      set({ unlistenFn: null });
    }
  },
}));
