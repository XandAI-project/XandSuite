import { useEffect, useRef, useCallback } from "react";
import { EyeOff, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { invoke } from "@/lib/tauri";
import { useBrowserAgentStore } from "@/stores/browserAgentStore";

/**
 * Renders the streamed Chromium viewport as a `<canvas>`.
 *
 * Frames never live in Zustand — the store hands them off to this component
 * through `registerFrameSink` and we decode + `drawImage` straight to the
 * canvas. Pointer / keyboard events are forwarded to the backend only when
 * the session is in take-over mode, matching the policy in
 * `forward_browser_mouse` / `forward_browser_key` on the Rust side.
 */
export function BrowserViewport() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const registerFrameSink = useBrowserAgentStore((s) => s.registerFrameSink);
  const viewportSize = useBrowserAgentStore((s) => s.viewportSize);
  const conversationId = useBrowserAgentStore((s) => s.conversationId);
  const takeover = useBrowserAgentStore((s) => s.takeover);
  const stealth = useBrowserAgentStore((s) => s.stealth);
  const setStealth = useBrowserAgentStore((s) => s.setStealth);
  const sessionId = useBrowserAgentStore((s) => s.sessionId);
  const url = useBrowserAgentStore((s) => s.url);
  const title = useBrowserAgentStore((s) => s.title);
  const loadState = useBrowserAgentStore((s) => s.loadState);

  useEffect(() => {
    // Skip registering the frame sink entirely in stealth mode — the backend
    // isn't streaming anyway, but bailing out here prevents a stale listener
    // from silently holding on to an Image element and blitting half-decoded
    // frames if the stream is ever resumed mid-flight.
    if (stealth) return;
    imgRef.current = new Image();
    const unsub = registerFrameSink((frame) => {
      const canvas = canvasRef.current;
      const img = imgRef.current;
      if (!canvas || !img) return;
      if (canvas.width !== frame.width || canvas.height !== frame.height) {
        canvas.width = frame.width;
        canvas.height = frame.height;
      }
      img.onload = () => {
        const ctx = canvas.getContext("2d");
        if (!ctx) return;
        ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
      };
      img.src = `data:image/jpeg;base64,${frame.data_base64}`;
    });
    return unsub;
  }, [registerFrameSink, stealth]);

  /** Map a CSS-space pointer event to Chromium viewport coordinates. */
  const toViewportCoords = useCallback(
    (e: React.PointerEvent<HTMLCanvasElement> | React.MouseEvent<HTMLCanvasElement>) => {
      const canvas = canvasRef.current;
      if (!canvas) return { x: 0, y: 0 };
      const rect = canvas.getBoundingClientRect();
      const sx = canvas.width / Math.max(rect.width, 1);
      const sy = canvas.height / Math.max(rect.height, 1);
      return {
        x: (e.clientX - rect.left) * sx,
        y: (e.clientY - rect.top) * sy,
      };
    },
    []
  );

  const dispatchMouse = useCallback(
    async (
      kind: "move" | "down" | "up" | "wheel",
      e: React.PointerEvent<HTMLCanvasElement> | React.MouseEvent<HTMLCanvasElement>
    ) => {
      if (!conversationId || !takeover) return;
      const { x, y } = toViewportCoords(e);
      const buttonMap: Record<number, "left" | "right" | "middle"> = {
        0: "left",
        1: "middle",
        2: "right",
      };
      const button = "button" in e ? buttonMap[e.button] ?? "left" : undefined;
      try {
        await invoke("forward_browser_mouse", {
          conversationId,
          kind,
          x,
          y,
          button,
          clickCount: kind === "down" ? 1 : undefined,
        });
      } catch {
        /* swallow — takeover edge transitions can race cleanup */
      }
    },
    [conversationId, takeover, toViewportCoords]
  );

  const dispatchKey = useCallback(
    async (kind: "down" | "up" | "char", e: React.KeyboardEvent<HTMLCanvasElement>) => {
      if (!conversationId || !takeover) return;
      try {
        await invoke("forward_browser_key", {
          conversationId,
          kind,
          key: e.key,
          code: e.code,
          text: kind === "char" && e.key.length === 1 ? e.key : undefined,
        });
      } catch {
        /* ignore */
      }
    },
    [conversationId, takeover]
  );

  if (stealth) {
    return (
      <div className="flex-1 min-h-0 relative bg-gradient-to-br from-background to-muted/40 flex items-center justify-center p-6">
        <div className="max-w-sm w-full text-center space-y-4">
          <div className="mx-auto w-14 h-14 rounded-full bg-muted flex items-center justify-center">
            <EyeOff className="w-7 h-7 text-muted-foreground" />
          </div>
          <div className="space-y-1">
            <h3 className="text-sm font-semibold">Stealth mode</h3>
            <p className="text-xs text-muted-foreground leading-relaxed">
              The browser is hidden and the screencast is paused to save CPU
              and bandwidth. {sessionId ? (
                <>
                  The agent is <span className="text-emerald-500 font-medium">still running</span>{" "}
                  — it will keep navigating, clicking, and reporting back in the
                  chat.
                </>
              ) : (
                <>
                  Start a session and the agent will execute tasks without
                  streaming the viewport.
                </>
              )}
            </p>
          </div>
          {sessionId && (
            <div className="rounded-md border border-border bg-card/70 px-3 py-2 text-left space-y-1">
              <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
                {loadState === "loading" ? (
                  <>
                    <Loader2 className="w-3 h-3 animate-spin" />
                    <span>loading…</span>
                  </>
                ) : (
                  <>
                    <span className="w-2 h-2 rounded-full bg-emerald-400" />
                    <span>{loadState === "complete" ? "ready" : "idle"}</span>
                  </>
                )}
              </div>
              <div className="text-xs font-medium truncate" title={title}>
                {title || "Untitled page"}
              </div>
              <div className="text-[11px] font-mono text-muted-foreground truncate" title={url}>
                {url}
              </div>
            </div>
          )}
          <Button
            size="sm"
            variant="outline"
            onClick={() => void setStealth(false)}
            className="gap-1.5"
          >
            <EyeOff className="w-4 h-4" />
            Show browser
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div
      className="flex-1 min-h-0 relative bg-black"
      style={{ cursor: takeover ? "crosshair" : "default" }}
    >
      <canvas
        ref={canvasRef}
        width={viewportSize.width}
        height={viewportSize.height}
        className="w-full h-full"
        tabIndex={0}
        onPointerMove={(e) => void dispatchMouse("move", e)}
        onPointerDown={(e) => {
          canvasRef.current?.focus();
          void dispatchMouse("down", e);
        }}
        onPointerUp={(e) => void dispatchMouse("up", e)}
        onKeyDown={(e) => {
          void dispatchKey("down", e);
          if (e.key.length === 1) void dispatchKey("char", e);
        }}
        onKeyUp={(e) => void dispatchKey("up", e)}
        onContextMenu={(e) => e.preventDefault()}
      />
      {!takeover && (
        <div className="pointer-events-none absolute top-2 right-2 px-2 py-1 rounded bg-black/60 text-white text-[10px] uppercase tracking-wide">
          Agent control
        </div>
      )}
    </div>
  );
}
