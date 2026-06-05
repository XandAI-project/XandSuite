import { useEffect } from "react";
import { invoke } from "@/lib/tauri";
import { useBrowserAgentStore } from "@/stores/browserAgentStore";

/**
 * Global "kill switch" for the Browser Agent: Ctrl+Shift+X (Cmd+Shift+X on
 * macOS) immediately pauses the active session, blocking further CDP input
 * dispatch until the user resumes. Works regardless of which tab the user is
 * on — the listener is mounted once at the app root.
 *
 * No-ops when there is no active session so the shortcut is safe to enable
 * globally.
 */
export function useBrowserKillSwitch() {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (!(mod && e.shiftKey && (e.key === "X" || e.key === "x"))) return;
      const { conversationId, sessionId, paused } = useBrowserAgentStore.getState();
      if (!conversationId || !sessionId) return;
      e.preventDefault();
      e.stopPropagation();
      (async () => {
        try {
          if (paused) {
            await invoke("resume_browser_session", { conversationId });
            useBrowserAgentStore.setState({ paused: false });
          } else {
            await invoke("pause_browser_session", { conversationId });
            useBrowserAgentStore.setState({ paused: true });
          }
        } catch (err) {
          // Non-fatal — leave store state untouched.
          console.warn("[browser-kill-switch] toggle failed:", err);
        }
      })();
    };
    window.addEventListener("keydown", handler, { capture: true });
    return () => window.removeEventListener("keydown", handler, { capture: true });
  }, []);
}
