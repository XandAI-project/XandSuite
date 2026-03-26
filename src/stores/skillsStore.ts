import { create } from "zustand";
import { invoke, listen } from "@/lib/tauri";
import type {
  McpServerConfig,
  TaggedTool,
  ToolCallEvent,
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
  isLoading: boolean;
  error: string | null;

  fetchServers: () => Promise<void>;
  fetchTools: () => Promise<void>;
  addMcpServer: (req: AddServerRequest) => Promise<void>;
  removeMcpServer: (serverId: string) => Promise<void>;
  reloadBuiltins: () => Promise<void>;
  toggleSkills: () => void;
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

// Global listeners for tool-call events (set up once)
let toolCallUnlisten: (() => void) | null = null;
let toolResultUnlisten: (() => void) | null = null;

export const useSkillsStore = create<SkillsStore>((set, get) => {
  // Set up event listeners immediately
  listen<ToolCallEvent>("chat_tool_call", (event) => {
    const p = event.payload;
    set((state) => ({
      activeToolSteps: [
        ...state.activeToolSteps,
        {
          tool_call_id: p.tool_call_id,
          function_name: p.function_name,
          arguments: p.arguments,
          turn: p.turn,
        },
      ],
    }));
  }).then((fn) => { toolCallUnlisten = fn; });

  listen<ToolResultEvent>("chat_tool_result", (event) => {
    const p = event.payload;
    set((state) => ({
      activeToolSteps: state.activeToolSteps.map((step) =>
        step.tool_call_id === p.tool_call_id
          ? { ...step, result: p.result }
          : step
      ),
    }));
  }).then((fn) => { toolResultUnlisten = fn; });

  return {
    servers: [],
    tools: [],
    skillsEnabled: true,
    activeToolSteps: [],
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

    clearToolSteps: () => set({ activeToolSteps: [] }),

    clearError: () => set({ error: null }),
  };
});
