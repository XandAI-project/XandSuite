import { create } from "zustand";
import { invoke } from "../lib/tauri";
import { listen } from "@tauri-apps/api/event";
import type {
  CodingEvent,
  CodingMessage,
  CodingMode,
  CodingPlan,
  CodingSession,
  FileTreeEntry,
} from "../lib/tauri";

// ── Live event for the current run ───────────────────────────────────────────

export interface LiveCodingEvent {
  event_type: string;
  payload: Record<string, unknown>;
  timestamp: number;
}

// ── Store ─────────────────────────────────────────────────────────────────────

interface CodingStore {
  // Project
  projectPath: string | null;
  fileTree: FileTreeEntry[];
  fileTreeLoading: boolean;
  openFile: string | null;
  openFileContent: string | null;

  // Sessions
  sessions: CodingSession[];
  activeSession: CodingSession | null;
  messages: CodingMessage[];

  // Current run state
  mode: CodingMode;
  isRunning: boolean;
  liveEvents: LiveCodingEvent[];
  currentPlan: CodingPlan | null;
  streamingContent: string;

  // Terminal output (from shell_exec observations)
  terminalOutput: string[];

  // UI state
  showPlanPanel: boolean;
  showTerminal: boolean;
  error: string | null;

  // Actions
  setProjectPath: (path: string | null) => void;
  selectProject: () => Promise<void>;
  loadFileTree: () => Promise<void>;
  openFilePreview: (filePath: string) => Promise<void>;
  setMode: (mode: CodingMode) => void;

  fetchSessions: () => Promise<void>;
  createSession: () => Promise<void>;
  openSession: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string) => Promise<void>;

  sendMessage: (content: string) => Promise<void>;
  executePlan: () => Promise<void>;
  cancelRun: () => Promise<void>;

  listenToEvents: () => Promise<() => void>;

  setShowPlanPanel: (show: boolean) => void;
  setShowTerminal: (show: boolean) => void;
  clearError: () => void;
}

export const useCodingStore = create<CodingStore>((set, get) => ({
  projectPath: null,
  fileTree: [],
  fileTreeLoading: false,
  openFile: null,
  openFileContent: null,

  sessions: [],
  activeSession: null,
  messages: [],

  mode: "agent",
  isRunning: false,
  liveEvents: [],
  currentPlan: null,
  streamingContent: "",

  terminalOutput: [],

  showPlanPanel: true,
  showTerminal: false,
  error: null,

  // ── Project ────────────────────────────────────────────────────────────────

  setProjectPath: (path) => {
    set({ projectPath: path, fileTree: [], openFile: null, openFileContent: null });
    if (path) {
      get().loadFileTree();
    }
  },

  selectProject: async () => {
    try {
      const path = await invoke<string | null>("select_coding_project");
      if (path) {
        get().setProjectPath(path);
        // Persist on active session too
        const { activeSession } = get();
        if (activeSession) {
          await invoke("update_coding_session", {
            sessionId: activeSession.id,
            projectPath: path,
          });
          set((s) => ({
            activeSession: s.activeSession
              ? { ...s.activeSession, project_path: path }
              : null,
          }));
        }
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  loadFileTree: async () => {
    const { projectPath } = get();
    if (!projectPath) return;
    set({ fileTreeLoading: true });
    try {
      const result = await invoke<{ tree: FileTreeEntry[] }>("list_coding_directory", {
        projectPath,
        subPath: null,
        depth: 4,
      });
      set({ fileTree: result.tree ?? [], fileTreeLoading: false });
    } catch (e) {
      set({ error: String(e), fileTreeLoading: false });
    }
  },

  openFilePreview: async (filePath) => {
    // Empty string = close the preview
    if (!filePath) {
      set({ openFile: null, openFileContent: null });
      return;
    }
    const { projectPath } = get();
    if (!projectPath) return;
    try {
      const content = await invoke<string>("read_coding_file", {
        projectPath,
        filePath,
      });
      set({ openFile: filePath, openFileContent: content });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setMode: (mode) => set({ mode }),

  // ── Sessions ───────────────────────────────────────────────────────────────

  fetchSessions: async () => {
    try {
      const sessions = await invoke<CodingSession[]>("list_coding_sessions");
      set({ sessions });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  createSession: async () => {
    const { mode, projectPath } = get();
    try {
      const session = await invoke<CodingSession>("create_coding_session", {
        mode,
        projectPath,
      });
      set((s) => ({
        sessions: [session, ...s.sessions],
        activeSession: session,
        messages: [],
        liveEvents: [],
        currentPlan: null,
        terminalOutput: [],
        streamingContent: "",
        isRunning: false,
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  openSession: async (sessionId) => {
    try {
      const [session, messages] = await invoke<[CodingSession, CodingMessage[]]>(
        "get_coding_session",
        { sessionId }
      );
      // Load plan if any
      let currentPlan: CodingPlan | null = null;
      try {
        currentPlan = await invoke<CodingPlan | null>("get_coding_plan", { sessionId });
      } catch {
        // No plan yet
      }
      set({
        activeSession: session,
        messages,
        currentPlan,
        liveEvents: [],
        isRunning: false,
        streamingContent: "",
        terminalOutput: [],
        // Restore project path from session
        projectPath: session.project_path ?? get().projectPath,
        mode: (session.mode as CodingMode) ?? get().mode,
      });
      if (session.project_path) {
        get().loadFileTree();
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  deleteSession: async (sessionId) => {
    try {
      await invoke("delete_coding_session", { sessionId });
      set((s) => {
        const sessions = s.sessions.filter((x) => x.id !== sessionId);
        const activeSession =
          s.activeSession?.id === sessionId ? null : s.activeSession;
        return { sessions, activeSession, messages: activeSession ? s.messages : [] };
      });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  // ── Messaging ──────────────────────────────────────────────────────────────

  sendMessage: async (content) => {
    const { activeSession, mode } = get();

    // Auto-create session if none
    let session = activeSession;
    if (!session) {
      await get().createSession();
      session = get().activeSession;
      if (!session) return;
    }

    // Sync mode to DB if the user switched it before sending
    if (session.mode !== mode) {
      await invoke("update_coding_session", {
        sessionId: session.id,
        title: null,
        mode,
        projectPath: null,
      });
      // Keep local activeSession in sync so subsequent checks are correct
      set((s) => ({
        activeSession: s.activeSession ? { ...s.activeSession, mode } : s.activeSession,
      }));
    }

    set({
      isRunning: true,
      liveEvents: [],
      streamingContent: "",
      error: null,
    });

    try {
      const userMsg = await invoke<CodingMessage>("send_coding_message", {
        sessionId: session.id,
        content,
      });
      set((s) => ({ messages: [...s.messages, userMsg] }));
    } catch (e) {
      set({ error: String(e), isRunning: false });
    }
  },

  executePlan: async () => {
    const { currentPlan, sendMessage } = get();
    if (!currentPlan) return;
    set({ mode: "agent" });
    const taskList = currentPlan.tasks
      .map((t, i) => `${i + 1}. ${t.title}: ${t.description}`)
      .join("\n");
    await sendMessage(
      `Execute the following plan tasks one by one using your tools.\n` +
      `Use file_write to create or edit files, shell_exec to run commands, ` +
      `and update_task to mark each task complete as you finish it.\n\n` +
      `Tasks:\n${taskList}`
    );
  },

  cancelRun: async () => {
    const { activeSession } = get();
    if (!activeSession) return;
    try {
      await invoke("cancel_coding_session", { sessionId: activeSession.id });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  // ── Event listener ─────────────────────────────────────────────────────────

  listenToEvents: async () => {
    const unlisten = await listen<CodingEvent>("coding_event", (event) => {
      const ev = event.payload as CodingEvent;
      const { event_type, payload, session_id } = ev;

      const { activeSession } = get();
      if (activeSession && activeSession.id !== session_id) return;

      const liveEvent: LiveCodingEvent = {
        event_type,
        payload,
        timestamp: Date.now(),
      };

      set((s) => {
        const liveEvents = [...s.liveEvents, liveEvent];

        // Terminal output: capture shell_exec observations
        let terminalOutput = s.terminalOutput;
        if (
          event_type === "observation" &&
          payload.tool === "shell_exec" &&
          !payload.error
        ) {
          const obs = payload.observation as string;
          terminalOutput = [...terminalOutput, obs];
        }

        // Plan updates
        let currentPlan = s.currentPlan;
        if (event_type === "plan_created" || event_type === "task_updated") {
          currentPlan = payload as unknown as CodingPlan;
        }

        // Completed / failed / cancelled — stop running, save assistant message
        if (
          event_type === "completed" ||
          event_type === "failed" ||
          event_type === "cancelled"
        ) {
          const answer = (payload.answer ?? payload.reason ?? "") as string;
          const assistantMsg: CodingMessage = {
            id: `live-${Date.now()}`,
            session_id: session_id,
            role: "assistant",
            content: answer,
            events: liveEvents.map((e) => ({
              event_type: e.event_type,
              payload: e.payload,
            })),
            created_at: new Date().toISOString(),
          };
          return {
            liveEvents,
            currentPlan,
            terminalOutput,
            isRunning: false,
            messages: [...s.messages, assistantMsg],
            streamingContent: "",
          };
        }

        return { liveEvents, currentPlan, terminalOutput };
      });
    });

    return unlisten;
  },

  // ── UI ─────────────────────────────────────────────────────────────────────

  setShowPlanPanel: (show) => set({ showPlanPanel: show }),
  setShowTerminal: (show) => set({ showTerminal: show }),
  clearError: () => set({ error: null }),
}));
