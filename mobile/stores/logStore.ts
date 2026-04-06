import { create } from "zustand";
import { logsApi } from "../api/endpoints";
import { sseManager } from "../api/sse";
import { LogEntry } from "../lib/types";

const MAX_LOGS = 500;

interface LogState {
  logs: LogEntry[];
  fetchLogs: () => Promise<void>;
  clear: () => void;
}

export const useLogStore = create<LogState>((set) => {
  sseManager.on("app_log", (event) => {
    if (event.type !== "app_log") return;
    const entry: LogEntry = {
      level: event.level as LogEntry["level"],
      message: event.message,
      ts: event.ts,
    };
    set((s) => ({
      logs: [entry, ...s.logs].slice(0, MAX_LOGS),
    }));
  });

  return {
    logs: [],

    fetchLogs: async () => {
      const logs = await logsApi.get();
      set({ logs: [...logs].reverse() });
    },

    clear: () => set({ logs: [] }),
  };
});
