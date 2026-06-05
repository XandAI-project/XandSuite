import { useEffect, useState } from "react";
import { useBrowserAgentStore } from "@/stores/browserAgentStore";

/**
 * Thin status strip along the bottom of the viewport column. Mirrors what
 * Operator / Browser Use show: load state, title, frame rate, pause/takeover
 * flags. No interactive controls — the toolbar owns those.
 */
export function BrowserStatusBar() {
  const url = useBrowserAgentStore((s) => s.url);
  const title = useBrowserAgentStore((s) => s.title);
  const loadState = useBrowserAgentStore((s) => s.loadState);
  const paused = useBrowserAgentStore((s) => s.paused);
  const takeover = useBrowserAgentStore((s) => s.takeover);
  const lastFrameAt = useBrowserAgentStore((s) => s.lastFrameAt);
  const streaming = useBrowserAgentStore((s) => s.streaming);
  const stealth = useBrowserAgentStore((s) => s.stealth);
  const sessionSource = useBrowserAgentStore((s) => s.sessionSource);

  const [fps, setFps] = useState(0);

  useEffect(() => {
    if (!streaming) {
      setFps(0);
      return;
    }
    // Rolling 1s fps estimate from frame timestamps. Simple — fine for a pill.
    let frames = 0;
    let last = performance.now();
    const unsub = useBrowserAgentStore.subscribe((s, prev) => {
      if (s.lastFrameAt !== prev.lastFrameAt) frames += 1;
    });
    const id = window.setInterval(() => {
      const now = performance.now();
      const dt = (now - last) / 1000;
      last = now;
      setFps(dt > 0 ? Math.round(frames / dt) : 0);
      frames = 0;
    }, 1000);
    return () => {
      unsub();
      window.clearInterval(id);
    };
  }, [streaming]);

  const loadPill = (() => {
    switch (loadState) {
      case "loading":
        return <span className="text-amber-400">loading…</span>;
      case "complete":
        return <span className="text-emerald-400">ready</span>;
      default:
        return <span className="text-muted-foreground">idle</span>;
    }
  })();

  return (
    <div className="h-7 flex items-center gap-3 px-3 border-t border-border bg-card/70 text-[11px] text-muted-foreground shrink-0">
      <span className="truncate max-w-[40%]" title={title}>
        {title || "—"}
      </span>
      <span className="truncate flex-1 min-w-0 font-mono" title={url}>
        {url}
      </span>
      {paused && (
        <span className="px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-400 text-[10px]">
          paused
        </span>
      )}
      {takeover && (
        <span className="px-1.5 py-0.5 rounded bg-sky-500/20 text-sky-400 text-[10px]">
          takeover
        </span>
      )}
      {stealth && (
        <span
          className="px-1.5 py-0.5 rounded bg-violet-500/20 text-violet-400 text-[10px]"
          title="Viewport hidden; agent is still running"
        >
          stealth
        </span>
      )}
      {sessionSource === "llm" && (
        <span
          className="px-1.5 py-0.5 rounded bg-indigo-500/20 text-indigo-400 text-[10px]"
          title="The AI launched this browser session on its own"
        >
          auto-launched
        </span>
      )}
      <span className="text-[10px] tabular-nums">
        {loadPill}
        {streaming && (
          <>
            <span className="mx-1.5 text-muted-foreground/50">•</span>
            <span>{fps} fps</span>
          </>
        )}
        {lastFrameAt && streaming && (
          <>
            <span className="mx-1.5 text-muted-foreground/50">•</span>
            <span>{Math.max(0, Math.round((Date.now() - lastFrameAt) / 100) / 10)}s ago</span>
          </>
        )}
      </span>
    </div>
  );
}
