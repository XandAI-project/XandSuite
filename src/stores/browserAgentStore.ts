import { create } from "zustand";
import { invoke, listen } from "@/lib/tauri";
import type {
  BrowserAgentConfirmRequestEvent,
  BrowserAgentFrameEvent,
  BrowserAgentLoadStateEvent,
  BrowserAgentSessionStartedEvent,
  BrowserAgentTitleEvent,
  BrowserAgentUrlEvent,
  BrowserSessionStatus,
  UnlistenFn,
} from "@/lib/tauri";

/**
 * Callback invoked on every incoming screencast frame. Registered by the
 * `BrowserViewport` component so the canvas renderer receives frames without
 * them being persisted in Zustand (frames are VERY hot — 10-15 fps of base64
 * JPEG data would wreck React re-render cost).
 */
export type FrameSink = (frame: BrowserAgentFrameEvent) => void;

interface BrowserAgentStore {
  /** The conversation id currently bound to the viewport. `null` until the user opens the tab. */
  conversationId: string | null;
  /** The Rust-side `session_id` once `start_browser_session` returns. */
  sessionId: string | null;
  /**
   * Who launched the current session — `"user"` (toolbar click) or `"llm"`
   * (the agent called `browser_agent__start_session` itself). Used by the
   * status bar to show a small attribution hint.
   */
  sessionSource: "user" | "llm" | null;
  url: string;
  title: string;
  loadState: "idle" | "loading" | "complete";
  paused: boolean;
  takeover: boolean;
  /** Last known viewport size in CSS pixels — drives canvas scaling. */
  viewportSize: { width: number; height: number };
  /** Last time a frame arrived (ms since epoch); used for the fps pill. */
  lastFrameAt: number | null;
  /** Pending confirmation dialog, if any. */
  pendingConfirm: BrowserAgentConfirmRequestEvent | null;
  /** Whether the backend is actively streaming frames. */
  streaming: boolean;
  /** Whether a start/stop is in flight so UI can disable toolbar buttons. */
  busy: boolean;
  error: string | null;

  /** Cookie session id selected from Settings → Browser, replayed on launch. */
  cookieSessionId: string | null;
  /**
   * Stealth mode — hides the embedded viewport on the UI side and stops the
   * screencast on the backend side. The underlying browser session keeps
   * running; this is purely a presentation + bandwidth optimisation. Persisted
   * across reloads in localStorage under `browserStealth`.
   */
  stealth: boolean;
  setConversationId: (id: string | null) => void;
  setCookieSessionId: (id: string | null) => void;
  setStealth: (value: boolean) => Promise<void>;
  startSession: (
    conversationId: string,
    opts?: {
      profileName?: string;
      initialUrl?: string;
      chromeExecutable?: string;
      cookieSessionId?: string | null;
    }
  ) => Promise<void>;
  stopSession: () => Promise<void>;
  startScreencast: () => Promise<void>;
  stopScreencast: () => Promise<void>;
  togglePause: () => Promise<void>;
  setTakeover: (value: boolean) => Promise<void>;
  navigate: (url: string) => Promise<void>;
  resolveConfirmation: (requestId: string, approved: boolean) => void;
  /** Register the canvas-side frame sink. Returns an unregister function. */
  registerFrameSink: (sink: FrameSink) => () => void;
  /** Start / stop Tauri event listeners. Called by the tab on mount / unmount. */
  attachListeners: () => Promise<() => void>;
}

const frameSinks = new Set<FrameSink>();

const readStealthPref = (): boolean => {
  try {
    return localStorage.getItem("browserStealth") === "1";
  } catch {
    return false;
  }
};
const writeStealthPref = (value: boolean) => {
  try {
    if (value) localStorage.setItem("browserStealth", "1");
    else localStorage.removeItem("browserStealth");
  } catch {
    /* quota / sandbox — in-memory state still reflects the toggle */
  }
};

export const useBrowserAgentStore = create<BrowserAgentStore>((set, get) => ({
  conversationId: null,
  sessionId: null,
  sessionSource: null,
  url: "about:blank",
  title: "",
  loadState: "idle",
  paused: false,
  takeover: false,
  viewportSize: { width: 1280, height: 800 },
  lastFrameAt: null,
  pendingConfirm: null,
  streaming: false,
  busy: false,
  error: null,
  cookieSessionId: null,
  stealth: readStealthPref(),

  setConversationId: (id) => set({ conversationId: id }),
  setCookieSessionId: (id) => set({ cookieSessionId: id }),

  setStealth: async (value) => {
    const prev = get().stealth;
    if (prev === value) return;
    writeStealthPref(value);
    set({ stealth: value });

    // Propagate to the backend screencast so we actually save bandwidth when
    // the user can't see the viewport anyway. The session itself keeps
    // running — the agent loop is entirely independent from screencasting.
    const { conversationId, sessionId, streaming } = get();
    if (!conversationId || !sessionId) return;
    try {
      if (value && streaming) {
        await invoke("stop_browser_screencast", { conversationId });
        set({ streaming: false });
      } else if (!value && !streaming) {
        await invoke("start_browser_screencast", {
          conversationId,
          quality: 60,
          maxWidth: 1280,
          maxHeight: 800,
          everyNthFrame: 2,
        });
        set({ streaming: true });
      }
    } catch (e) {
      // Screencast start/stop failures are non-fatal; surface through error
      // slot so the toolbar's streaming dot still signals reality.
      console.warn("[browser-agent] stealth screencast toggle failed:", e);
    }
  },

  startSession: async (conversationId, opts) => {
    set({ busy: true, error: null, conversationId });
    try {
      // Caller-supplied cookie session takes precedence over the sticky one;
      // explicit `null` clears.
      const cookieSessionId =
        opts && "cookieSessionId" in opts
          ? opts.cookieSessionId ?? null
          : get().cookieSessionId;
      const res = await invoke<{
        session_id: string;
        reused: boolean;
        cookies_applied?: number;
      }>("start_browser_session", {
        conversationId,
        profileName: opts?.profileName,
        initialUrl: opts?.initialUrl ?? "about:blank",
        chromeExecutable: opts?.chromeExecutable,
        cookieSessionId,
      });
      if ((res.cookies_applied ?? 0) > 0) {
        console.info(
          `[browser-agent] applied ${res.cookies_applied} cookies on launch`
        );
      }
      set({ sessionId: res.session_id, sessionSource: "user" });
      // Hydrate status so the toolbar URL/title is populated on first open.
      const status = await invoke<BrowserSessionStatus | null>(
        "get_browser_session_status",
        { conversationId }
      );
      if (status) {
        set({
          url: status.url || "about:blank",
          title: status.title || "",
          paused: status.paused,
          takeover: status.takeover,
        });
      }
      // Auto-start streaming so the canvas shows frames immediately, UNLESS
      // the user has opted into stealth mode — in which case we keep the
      // backend quiet until they flip the toggle. The embedded viewport is
      // the only way the user sees the headless browser, so we normally
      // wouldn't require a second manual click.
      if (!get().stealth) {
        try {
          await invoke("start_browser_screencast", {
            conversationId,
            quality: 60,
            maxWidth: 1280,
            maxHeight: 800,
            everyNthFrame: 2,
          });
          set({ streaming: true });
        } catch (e) {
          // Non-fatal — the user can retry from the toolbar's streaming toggle.
          console.warn("[browser-agent] auto-start screencast failed:", e);
        }
      }
    } catch (e) {
      set({ error: String(e) });
      throw e;
    } finally {
      set({ busy: false });
    }
  },

  stopSession: async () => {
    const { conversationId } = get();
    if (!conversationId) return;
    set({ busy: true });
    try {
      await invoke("stop_browser_screencast", { conversationId });
      await invoke("stop_browser_session", { conversationId });
    } finally {
      set({
        busy: false,
        sessionId: null,
        sessionSource: null,
        streaming: false,
        paused: false,
        takeover: false,
        lastFrameAt: null,
      });
    }
  },

  startScreencast: async () => {
    const { conversationId, streaming } = get();
    if (!conversationId || streaming) return;
    await invoke("start_browser_screencast", {
      conversationId,
      quality: 60,
      maxWidth: 1280,
      maxHeight: 800,
      everyNthFrame: 2,
    });
    set({ streaming: true });
  },

  stopScreencast: async () => {
    const { conversationId } = get();
    if (!conversationId) return;
    await invoke("stop_browser_screencast", { conversationId });
    set({ streaming: false });
  },

  togglePause: async () => {
    const { conversationId, paused } = get();
    if (!conversationId) return;
    const cmd = paused ? "resume_browser_session" : "pause_browser_session";
    await invoke(cmd, { conversationId });
    set({ paused: !paused });
  },

  setTakeover: async (value) => {
    const { conversationId } = get();
    if (!conversationId) return;
    await invoke("set_browser_takeover", { conversationId, takeover: value });
    set({ takeover: value });
  },

  navigate: async (url) => {
    const { conversationId } = get();
    if (!conversationId) return;
    try {
      await invoke("browser_toolbar_navigate", { conversationId, url });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  resolveConfirmation: (requestId, approved) => {
    // Post the response via a Tauri event so the backend's SafetyGate waiter
    // can resume. The backend listens for `browser_agent_confirm_response`.
    const { pendingConfirm } = get();
    if (!pendingConfirm || pendingConfirm.request_id !== requestId) return;
    // Use global listen API to emit from the renderer is not standard; use invoke instead.
    // NB: a dedicated `respond_browser_confirm` command will ship with the
    // post-MVP safety-gate wiring. For now we just clear the prompt.
    set({ pendingConfirm: null });
    void approved;
  },

  registerFrameSink: (sink) => {
    frameSinks.add(sink);
    return () => {
      frameSinks.delete(sink);
    };
  },

  attachListeners: async () => {
    const unlisteners: UnlistenFn[] = [];

    unlisteners.push(
      await listen<BrowserAgentFrameEvent>("browser_agent_frame", (ev) => {
        const f = ev.payload;
        set((s) =>
          s.viewportSize.width !== f.width || s.viewportSize.height !== f.height
            ? {
                viewportSize: { width: f.width, height: f.height },
                lastFrameAt: f.ts_ms,
              }
            : { lastFrameAt: f.ts_ms }
        );
        // Frames never enter Zustand — hand off directly to the canvas.
        frameSinks.forEach((sink) => sink(f));
      })
    );

    unlisteners.push(
      await listen<BrowserAgentUrlEvent>("browser_agent_url", (ev) => {
        set({ url: ev.payload.url });
      })
    );

    unlisteners.push(
      await listen<BrowserAgentTitleEvent>("browser_agent_title", (ev) => {
        set({ title: ev.payload.title });
      })
    );

    unlisteners.push(
      await listen<BrowserAgentLoadStateEvent>("browser_agent_load_state", (ev) => {
        const s = ev.payload.state === "complete" ? "complete" : "loading";
        set({ loadState: s });
      })
    );

    unlisteners.push(
      await listen<BrowserAgentConfirmRequestEvent>(
        "browser_agent_confirm_request",
        (ev) => {
          set({ pendingConfirm: ev.payload });
        }
      )
    );

    // LLM-initiated launches don't go through the `startSession` action, so
    // we rely on this event to sync the frontend session state and kick off
    // the screencast (unless the user has stealth mode on).
    unlisteners.push(
      await listen<BrowserAgentSessionStartedEvent>(
        "browser_agent_session_started",
        (ev) => {
          const p = ev.payload;
          const cur = get();
          // Only care about this conversation — registries are keyed by conv id.
          if (cur.conversationId && p.conversation_id !== cur.conversationId) {
            return;
          }
          const alreadyKnown = cur.sessionId === p.session_id;
          set({
            sessionId: p.session_id,
            sessionSource: p.source === "llm" ? "llm" : "user",
            url: p.url || "about:blank",
          });
          // Fire-and-forget screencast bootstrap for LLM launches — the
          // user-triggered path already started it inside `startSession`.
          if (
            !alreadyKnown &&
            p.source === "llm" &&
            !cur.stealth &&
            !cur.streaming
          ) {
            void (async () => {
              try {
                await invoke("start_browser_screencast", {
                  conversationId: p.conversation_id,
                  quality: 60,
                  maxWidth: 1280,
                  maxHeight: 800,
                  everyNthFrame: 2,
                });
                set({ streaming: true });
              } catch (e) {
                console.warn(
                  "[browser-agent] auto-start screencast after LLM launch failed:",
                  e
                );
              }
            })();
          }
        }
      )
    );

    return () => unlisteners.forEach((u) => u());
  },
}));
