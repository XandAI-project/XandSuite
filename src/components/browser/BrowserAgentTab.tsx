import { useEffect, useRef, useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { ChatView } from "@/components/chat/ChatView";
import { BrowserViewport } from "./BrowserViewport";
import { BrowserToolbar } from "./BrowserToolbar";
import { BrowserStatusBar } from "./BrowserStatusBar";
import { ConfirmActionModal } from "./ConfirmActionModal";
import { useBrowserAgentStore } from "@/stores/browserAgentStore";
import { useChatStore } from "@/stores/chatStore";
import { cn } from "@/lib/utils";

/**
 * Split-view tab combining the full `ChatView` (left) with an embedded
 * Chromium viewport (right). The two sides share a conversation id so the
 * agent loop and the browser session stay in lockstep: the backend uses that
 * id to key the per-session `BrowserSession` entry in `BrowserSessionRegistry`
 * and inject the "Browser Agent — ACTIVE" block into the system prompt.
 *
 * When stealth mode is active the right panel collapses entirely so the UI
 * looks like a normal chat. A thin tab/arrow on the right edge lets the user
 * slide the panel back open without leaving stealth (they can re-enable it
 * from the toolbar once the panel is visible again).
 */
export function BrowserAgentTab() {
  const activeConversation = useChatStore((s) => s.activeConversation);
  const createConversation = useChatStore((s) => s.createConversation);
  const openConversation = useChatStore((s) => s.openConversation);

  const setConversationId = useBrowserAgentStore((s) => s.setConversationId);
  const attachListeners = useBrowserAgentStore((s) => s.attachListeners);
  const stealth = useBrowserAgentStore((s) => s.stealth);
  const setStealth = useBrowserAgentStore((s) => s.setStealth);

  // Resizable split — percent width of the left (chat) column.
  const [leftPct, setLeftPct] = useState(45);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const draggingRef = useRef(false);

  // Track whether the browser panel is manually collapsed (independent of
  // stealth). Starts collapsed whenever stealth is on so the initial render
  // is already in the "just chat" state.
  const [panelOpen, setPanelOpen] = useState(!stealth);

  // Mirror stealth → panelOpen: switching stealth on should collapse the panel,
  // but switching it off just reveals the stealth placeholder — the user still
  // has to click the arrow to see the live viewport again (or they can use
  // "Show browser" inside BrowserViewport).
  useEffect(() => {
    if (stealth) setPanelOpen(false);
  }, [stealth]);

  // Ensure we have a conversation the moment the tab mounts so the user can
  // immediately type a browser task without first going to /chat.
  useEffect(() => {
    if (activeConversation) return;
    (async () => {
      try {
        const conv = await createConversation();
        await openConversation(conv.id);
      } catch {
        /* ignore — ChatView will show its own error states */
      }
    })();
  }, [activeConversation, createConversation, openConversation]);

  useEffect(() => {
    setConversationId(activeConversation?.id ?? null);
  }, [activeConversation?.id, setConversationId]);

  useEffect(() => {
    let cancelled = false;
    let detach: (() => void) | undefined;
    (async () => {
      const fn = await attachListeners();
      if (cancelled) {
        // The effect was cleaned up before attachListeners() resolved (e.g.
        // fast tab switch/unmount). `detach` was never assigned in time for
        // the cleanup below to call it, so the six listeners it registers
        // would otherwise leak for the lifetime of the app. Detach immediately.
        fn();
      } else {
        detach = fn;
      }
    })();
    return () => {
      cancelled = true;
      if (detach) detach();
    };
  }, [attachListeners]);

  const onDragStart = (e: React.PointerEvent) => {
    draggingRef.current = true;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };
  const onDragMove = (e: React.PointerEvent) => {
    if (!draggingRef.current || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    const pct = ((e.clientX - rect.left) / rect.width) * 100;
    setLeftPct(Math.min(75, Math.max(25, pct)));
  };
  const onDragEnd = (e: React.PointerEvent) => {
    draggingRef.current = false;
    (e.target as HTMLElement).releasePointerCapture(e.pointerId);
  };

  // Open the panel and, if stealth was the reason it was hidden, disable
  // stealth so the live viewport becomes visible immediately.
  const openPanel = () => {
    setPanelOpen(true);
    if (stealth) setStealth(false);
  };

  return (
    <div ref={containerRef} className="flex h-full w-full overflow-hidden relative">
      {/* Left column — full chat experience */}
      <div
        className={cn(
          "h-full min-w-0 transition-all duration-300",
          panelOpen ? "border-r border-border" : "border-r-0",
        )}
        style={{ width: panelOpen ? `${leftPct}%` : "100%" }}
      >
        <ChatView />
      </div>

      {/* Drag handle — only rendered when panel is open */}
      {panelOpen && (
        <div
          className="w-1 cursor-col-resize bg-border hover:bg-primary/40 transition-colors shrink-0"
          onPointerDown={onDragStart}
          onPointerMove={onDragMove}
          onPointerUp={onDragEnd}
        />
      )}

      {/* Right column — browser viewport (collapses when stealth+hidden) */}
      <div
        className={cn(
          "h-full flex flex-col overflow-hidden transition-all duration-300",
          panelOpen ? "min-w-0" : "w-0",
        )}
        style={{ width: panelOpen ? `${100 - leftPct - 0.25}%` : 0 }}
      >
        {panelOpen && (
          <>
            <BrowserToolbar conversationId={activeConversation?.id ?? null} />
            <BrowserViewport />
            <BrowserStatusBar />
          </>
        )}
      </div>

      {/* ── Collapse / expand toggle ──────────────────────────────────────────
          A thin pill anchored to the right edge of the left column. When the
          panel is open it sits on the drag handle; when collapsed it sits at
          the very right edge of the window so the user always has a way to
          bring the browser back.
      */}
      <button
        onClick={panelOpen ? () => setPanelOpen(false) : openPanel}
        title={panelOpen ? "Hide browser panel" : "Show browser panel"}
        className={cn(
          "absolute top-1/2 -translate-y-1/2 z-20",
          "flex items-center justify-center",
          "w-5 h-12 rounded-l-md",
          "bg-secondary border border-border border-r-0",
          "text-muted-foreground hover:text-foreground hover:bg-muted",
          "transition-all duration-300 shadow-sm",
        )}
        style={{
          right: panelOpen ? `calc(${100 - leftPct}% - 1px)` : 0,
        }}
      >
        {panelOpen ? (
          <ChevronRight className="w-3 h-3" />
        ) : (
          <ChevronLeft className="w-3 h-3" />
        )}
      </button>

      <ConfirmActionModal />
    </div>
  );
}
