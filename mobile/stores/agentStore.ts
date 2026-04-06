import { create } from "zustand";
import { agentApi } from "../api/endpoints";
import { sseManager } from "../api/sse";
import { AgentTask } from "../lib/types";

interface AgentEvent {
  task_id: string;
  event_type: string;
  payload: unknown;
}

interface AgentState {
  tasks: AgentTask[];
  activeEvents: Record<string, AgentEvent[]>;
  isLoading: boolean;

  fetchTasks: () => Promise<void>;
  runTask: (description: string) => Promise<string>;
  cancelTask: (id: string) => Promise<void>;
  deleteTask: (id: string) => Promise<void>;
}

export const useAgentStore = create<AgentState>((set, get) => {
  sseManager.on("agent_event", (event) => {
    if (event.type !== "agent_event") return;
    const agentEv: AgentEvent = {
      task_id: event.task_id,
      event_type: event.event_type,
      payload: event.payload,
    };
    set((s) => ({
      activeEvents: {
        ...s.activeEvents,
        [event.task_id]: [...(s.activeEvents[event.task_id] || []), agentEv],
      },
    }));
    if (event.event_type === "completed" || event.event_type === "failed") {
      setTimeout(() => get().fetchTasks(), 500);
    }
  });

  return {
    tasks: [],
    activeEvents: {},
    isLoading: false,

    fetchTasks: async () => {
      const tasks = await agentApi.list();
      set({ tasks });
    },

    runTask: async (description) => {
      const result = await agentApi.run(description.slice(0, 80), description);
      await get().fetchTasks();
      return result.task_id;
    },

    cancelTask: async (id) => {
      await agentApi.cancel(id);
      await get().fetchTasks();
    },

    deleteTask: async (id) => {
      await agentApi.delete(id);
      set((s) => ({ tasks: s.tasks.filter((t) => t.id !== id) }));
    },
  };
});
