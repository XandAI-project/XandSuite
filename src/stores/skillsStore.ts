import { create } from "zustand";
import { invoke, listen } from "@/lib/tauri";
import type {
  McpServerConfig,
  TaggedTool,
  ToolCallEvent,
  ToolCallPendingEvent,
  ToolResultEvent,
} from "@/lib/tauri";

// Re-export the backend ServerStatus shape under a clear name
export interface SkillServerStatus {
  config: McpServerConfig;
  connected: boolean;
  tool_count: number;
}

export interface ToolStep {
  tool_call_id: string;
  function_name: string;
  arguments: Record<string, unknown>;
  result?: string;
  turn: number;
}

interface SkillsStore {
  /** All connected servers and their status */
  servers: SkillServerStatus[];
  /** Flat list of all available tools across servers */
  tools: TaggedTool[];
  /** Whether skills are enabled for the current chat */
  skillsEnabled: boolean;
  /** Tool call steps for the current streaming message */
  activeToolSteps: ToolStep[];
  /**
   * Snapshot of tool steps from the last completed response.
   * Persists after streaming ends so the final message can still render
   * tool cards (e.g. generated images).  Cleared on the next send.
   */
  completedToolSteps: ToolStep[];
  isLoading: boolean;
  error: string | null;

  fetchServers: () => Promise<void>;
  fetchTools: () => Promise<void>;
  addMcpServer: (req: AddServerRequest) => Promise<void>;
  removeMcpServer: (serverId: string) => Promise<void>;
  reloadBuiltins: () => Promise<void>;
  toggleSkills: () => void;
  /** Snapshot activeToolSteps into completedToolSteps (called when streaming ends). */
  snapshotCompletedSteps: () => void;
  /** Clear both active and completed tool steps (called on new send or conv change). */
  clearToolSteps: () => void;
  clearError: () => void;
}

export interface AddServerRequest {
  id: string;
  name: string;
  description: string;
  transport: "stdio" | "http";
  command?: string;
  args?: string[];
  url?: string;
  auth?: string;
  icon?: string;
}

// Global listeners for tool-call events (set up once, kept for potential cleanup).
//
// Three events drive the tool-call UI lifecycle:
//   1. `chat_tool_call_pending` — fired while the LLM is still streaming the
//      call. Gives us an ID and function name so we can render an amber
//      "preparing…" card immediately. Arguments are not yet available.
//   2. `chat_tool_call`         — fired after streaming ends, right before
//      dispatch. Carries the full validated arguments; we merge them into
//      the existing step (or create one if the pending event was missed).
//   3. `chat_tool_result`       — fired when the tool process returns. We
//      attach the result to the matching step.
// All three key off `tool_call_id`; the store upserts by that id.

export const useSkillsStore = create<SkillsStore>((set, get) => {
  listen<ToolCallPendingEvent>("chat_tool_call_pending", (event) => {
    const p = event.payload;
    set((state) => {
      // If a full `chat_tool_call` somehow arrived first (unlikely but
      // harmless — network ordering is never guaranteed), keep that step.
      if (state.activeToolSteps.some((s) => s.tool_call_id === p.tool_call_id)) {
        return state;
      }
      return {
        activeToolSteps: [
          ...state.activeToolSteps,
          {
            tool_call_id: p.tool_call_id,
            function_name: p.function_name,
            arguments: {},
            turn: p.turn,
          },
        ],
      };
    });
  });

  listen<ToolCallEvent>("chat_tool_call", (event) => {
    const p = event.payload;
    set((state) => {
      const existing = state.activeToolSteps.find(
        (s) => s.tool_call_id === p.tool_call_id
      );
      if (existing) {
        return {
          activeToolSteps: state.activeToolSteps.map((step) =>
            step.tool_call_id === p.tool_call_id
              ? {
                  ...step,
                  function_name: p.function_name,
                  arguments: p.arguments,
                  turn: p.turn,
                }
              : step
          ),
        };
      }
      return {
        activeToolSteps: [
          ...state.activeToolSteps,
          {
            tool_call_id: p.tool_call_id,
            function_name: p.function_name,
            arguments: p.arguments,
            turn: p.turn,
          },
        ],
      };
    });
  });

  listen<ToolResultEvent>("chat_tool_result", (event) => {
    const p = event.payload;
    set((state) => ({
      activeToolSteps: state.activeToolSteps.map((step) =>
        step.tool_call_id === p.tool_call_id
          ? { ...step, result: p.result }
          : step
      ),
    }));
  });

  return {
    servers: [],
    tools: [],
    skillsEnabled: true,
    activeToolSteps: [],
    completedToolSteps: [],
    isLoading: false,
    error: null,

    fetchServers: async () => {
      try {
        const servers = await invoke<SkillServerStatus[]>("list_skill_servers");
        set({ servers });
      } catch (e) {
        set({ error: String(e) });
      }
    },

    fetchTools: async () => {
      set({ isLoading: true });
      try {
        const tools = await invoke<TaggedTool[]>("list_tools");
        set({ tools, isLoading: false });
      } catch (e) {
        set({ error: String(e), isLoading: false });
      }
    },

    addMcpServer: async (req: AddServerRequest) => {
      set({ isLoading: true, error: null });
      try {
        await invoke("add_mcp_server", { request: req });
        await get().fetchServers();
        await get().fetchTools();
        set({ isLoading: false });
      } catch (e) {
        set({ error: String(e), isLoading: false });
      }
    },

    removeMcpServer: async (serverId: string) => {
      try {
        await invoke("remove_mcp_server", { serverId });
        await get().fetchServers();
        await get().fetchTools();
      } catch (e) {
        set({ error: String(e) });
      }
    },

    reloadBuiltins: async () => {
      set({ isLoading: true });
      try {
        await invoke("reload_builtin_servers");
        await get().fetchServers();
        await get().fetchTools();
        set({ isLoading: false });
      } catch (e) {
        set({ error: String(e), isLoading: false });
      }
    },

    toggleSkills: () => set((s) => ({ skillsEnabled: !s.skillsEnabled })),

    snapshotCompletedSteps: () =>
      set((s) => ({ completedToolSteps: [...s.activeToolSteps] })),

    clearToolSteps: () => set({ activeToolSteps: [], completedToolSteps: [] }),

    clearError: () => set({ error: null }),
  };
});
