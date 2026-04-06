import { create } from "zustand";
import { sseManager } from "../api/sse";
import { skillsApi } from "../api/endpoints";
import { PersistedToolStep } from "../lib/types";

interface ToolStep {
  id: string;
  tool_call_id: string;
  function_name: string;
  arguments: Record<string, unknown>;
  result?: string | null;
  status: "pending" | "running" | "done" | "error";
  turn: number;
  gallery_id?: string | null;
  image_url?: string | null;
}

interface SkillsState {
  servers: unknown[];
  tools: unknown[];
  activeToolSteps: ToolStep[];
  completedToolSteps: ToolStep[];

  fetchServers: () => Promise<void>;
  fetchTools: () => Promise<void>;
  snapshotCompletedSteps: () => void;
  clearToolSteps: () => void;
}

export const useSkillsStore = create<SkillsState>((set, get) => {
  sseManager.on("chat_tool_call", (event) => {
    if (event.type !== "chat_tool_call") return;
    const step: ToolStep = {
      id: `${event.turn}_${event.tool_call_id}`,
      tool_call_id: event.tool_call_id,
      function_name: event.function_name,
      arguments: event.arguments as Record<string, unknown>,
      status: "running",
      turn: event.turn,
    };
    set((s) => ({ activeToolSteps: [...s.activeToolSteps, step] }));
  });

  sseManager.on("chat_tool_result", (event) => {
    if (event.type !== "chat_tool_result") return;
    set((s) => ({
      activeToolSteps: s.activeToolSteps.map((step) =>
        step.tool_call_id === event.tool_call_id
          ? { ...step, result: event.result, status: "done" }
          : step
      ),
    }));
  });

  return {
    servers: [],
    tools: [],
    activeToolSteps: [],
    completedToolSteps: [],

    fetchServers: async () => {
      const servers = await skillsApi.listServers();
      set({ servers });
    },

    fetchTools: async () => {
      const tools = await skillsApi.listTools();
      set({ tools });
    },

    snapshotCompletedSteps: () => {
      set((s) => ({ completedToolSteps: [...s.activeToolSteps] }));
    },

    clearToolSteps: () => {
      set({ activeToolSteps: [], completedToolSteps: [] });
    },
  };
});
