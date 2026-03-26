import { create } from "zustand";
import { invoke } from "../lib/tauri";
import { listen } from "@tauri-apps/api/event";
import type { AgentEvent, AgentTask } from "../lib/tauri";

interface AgentStore {
  tasks: AgentTask[];
  activeTask: AgentTask | null;
  /** IDs of tasks currently running */
  runningTaskIds: Set<string>;
  /** Live events keyed by task_id so multiple concurrent tasks work */
  liveEventsByTask: Record<string, AgentEvent[]>;
  error: string | null;

  // Derived helpers
  isRunning: (taskId?: string) => boolean;

  fetchTasks: () => Promise<void>;
  runTask: (description: string) => Promise<void>;
  deleteTask: (taskId: string) => Promise<void>;
  cancelTask: (taskId: string) => Promise<void>;
  setActiveTask: (task: AgentTask | null) => void;
  listenToEvents: () => Promise<() => void>;
  clearError: () => void;
}

export const useAgentStore = create<AgentStore>((set, get) => ({
  tasks: [],
  activeTask: null,
  runningTaskIds: new Set(),
  liveEventsByTask: {},
  error: null,

  isRunning: (taskId?: string) => {
    const { runningTaskIds } = get();
    if (taskId) return runningTaskIds.has(taskId);
    return runningTaskIds.size > 0;
  },

  fetchTasks: async () => {
    try {
      const tasks = await invoke<AgentTask[]>("list_agent_tasks");
      set((state) => {
        // Refresh activeTask if it's in the list
        const active = state.activeTask
          ? (tasks.find((t) => t.id === state.activeTask!.id) ?? state.activeTask)
          : null;
        return { tasks, activeTask: active };
      });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  runTask: async (description: string) => {
    set({ error: null });
    try {
      const task = await invoke<AgentTask>("run_agent_task", {
        taskDescription: description,
      });
      set((state) => ({
        tasks: [task, ...state.tasks],
        activeTask: task,
        runningTaskIds: new Set([...state.runningTaskIds, task.id]),
        liveEventsByTask: { ...state.liveEventsByTask, [task.id]: [] },
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  deleteTask: async (taskId: string) => {
    try {
      // Cancel first if still running so the background task stops cleanly
      if (get().runningTaskIds.has(taskId)) {
        try { await invoke("cancel_agent_task", { taskId }); } catch { /* best-effort */ }
      }
      await invoke("delete_agent_task", { taskId });
      set((state) => {
        const tasks = state.tasks.filter((t) => t.id !== taskId);
        const activeTask =
          state.activeTask?.id === taskId ? null : state.activeTask;
        const liveEventsByTask = { ...state.liveEventsByTask };
        delete liveEventsByTask[taskId];
        const runningTaskIds = new Set(state.runningTaskIds);
        runningTaskIds.delete(taskId);
        return { tasks, activeTask, liveEventsByTask, runningTaskIds };
      });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  cancelTask: async (taskId: string) => {
    try {
      await invoke("cancel_agent_task", { taskId });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setActiveTask: (task) => set({ activeTask: task }),

  listenToEvents: async () => {
    const unlisten = await listen<AgentEvent>("agent_event", (event) => {
      const ev = event.payload as AgentEvent;
      const { event_type, task_id } = ev;

      set((state) => {
        const prev = state.liveEventsByTask[task_id] ?? [];
        const liveEventsByTask = {
          ...state.liveEventsByTask,
          [task_id]: [...prev, ev],
        };

        let runningTaskIds = new Set(state.runningTaskIds);

        // When the task terminal event arrives, remove from running set and refresh
        if (
          event_type === "completed" ||
          event_type === "failed" ||
          event_type === "cancelled"
        ) {
          runningTaskIds.delete(task_id);
          // Update the matching task's status in the local list
          const tasks = state.tasks.map((t) =>
            t.id === task_id
              ? {
                  ...t,
                  status: event_type as AgentTask["status"],
                  result:
                    event_type === "completed"
                      ? String(ev.payload.result ?? t.result ?? "")
                      : t.result,
                }
              : t
          );
          const activeTask =
            state.activeTask?.id === task_id
              ? tasks.find((t) => t.id === task_id) ?? state.activeTask
              : state.activeTask;

          // Trigger a full refresh after a short delay for the DB to settle
          setTimeout(() => get().fetchTasks(), 500);

          return { liveEventsByTask, runningTaskIds, tasks, activeTask };
        }

        return { liveEventsByTask, runningTaskIds };
      });
    });
    return unlisten;
  },

  clearError: () => set({ error: null }),
}));
