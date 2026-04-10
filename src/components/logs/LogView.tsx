import { useEffect, useRef, useState, useMemo } from "react";
import { ScrollText, Trash2, ChevronDown, AlertCircle, Info, AlertTriangle, Bug } from "lucide-react";
import { useLogStore, type LogLevel } from "@/stores/logStore";
import { cn } from "@/lib/utils";

// ── Level metadata ────────────────────────────────────────────────────────────

const LEVEL_META: Record<LogLevel, { label: string; icon: React.ReactNode; pill: string; row: string }> = {
  error: {
    label: "Error",
    icon: <AlertCircle className="w-3 h-3" />,
    pill: "bg-red-500/20 text-red-400 border border-red-500/30",
    row: "border-l-2 border-red-500/40",
  },
  warn: {
    label: "Warn",
    icon: <AlertTriangle className="w-3 h-3" />,
    pill: "bg-yellow-500/20 text-yellow-400 border border-yellow-500/30",
    row: "border-l-2 border-yellow-500/40",
  },
  info: {
    label: "Info",
    icon: <Info className="w-3 h-3" />,
    pill: "bg-violet-500/10 text-violet-300/70 border border-violet-500/20",
    row: "border-l-2 border-transparent",
  },
  debug: {
    label: "Debug",
    icon: <Bug className="w-3 h-3" />,
    pill: "bg-secondary text-muted-foreground border border-border",
    row: "border-l-2 border-transparent",
  },
};

type Filter = "all" | LogLevel;

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all",   label: "All"   },
  { id: "error", label: "Error" },
  { id: "warn",  label: "Warn"  },
  { id: "info",  label: "Info"  },
  { id: "debug", label: "Debug" },
];

function formatTime(iso: string) {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  } catch {
    return iso;
  }
}

// ── LogView ───────────────────────────────────────────────────────────────────

export function LogView() {
  const entries    = useLogStore((s) => s.entries);
  const clearLogs  = useLogStore((s) => s.clear);

  const [filter, setFilter]       = useState<Filter>("all");
  const [autoScroll, setAutoScroll] = useState(true);

  const bottomRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(
    () => (filter === "all" ? entries : entries.filter((e) => e.level === filter)),
    [entries, filter],
  );

  // Count by level for badges
  const counts = useMemo(
    () =>
      entries.reduce<Record<LogLevel, number>>(
        (acc, e) => { acc[e.level] = (acc[e.level] ?? 0) + 1; return acc; },
        { info: 0, warn: 0, error: 0, debug: 0 },
      ),
    [entries],
  );

  // Auto-scroll to the bottom (newest entries are prepended, so scroll to top)
  useEffect(() => {
    if (autoScroll && entries.length > 0) {
      bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [entries, autoScroll]);

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Header */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border shrink-0">
        <ScrollText className="w-4 h-4 text-muted-foreground" />
        <span className="font-semibold text-sm">Logs</span>
        {entries.length > 0 && (
          <span className="ml-1 px-1.5 py-0.5 rounded-full bg-secondary text-muted-foreground text-[10px] font-medium">
            {entries.length}
          </span>
        )}
        {counts.error > 0 && (
          <span className="px-1.5 py-0.5 rounded-full bg-red-500/20 text-red-400 text-[10px] font-medium border border-red-500/30">
            {counts.error} error{counts.error !== 1 ? "s" : ""}
          </span>
        )}
        {counts.warn > 0 && (
          <span className="px-1.5 py-0.5 rounded-full bg-yellow-500/20 text-yellow-400 text-[10px] font-medium border border-yellow-500/30">
            {counts.warn} warn{counts.warn !== 1 ? "s" : ""}
          </span>
        )}

        <div className="ml-auto flex items-center gap-1.5">
          {/* Auto-scroll toggle */}
          <button
            onClick={() => setAutoScroll((v) => !v)}
            title={autoScroll ? "Auto-scroll on" : "Auto-scroll off"}
            className={cn(
              "flex items-center gap-1 px-2 py-1 rounded text-xs transition-colors",
              autoScroll
                ? "bg-primary/20 text-primary border border-primary/30"
                : "text-muted-foreground hover:bg-secondary",
            )}
          >
            <ChevronDown className="w-3 h-3" />
            Auto
          </button>

          {/* Clear button */}
          <button
            onClick={clearLogs}
            title="Clear logs"
            className="flex items-center gap-1 px-2 py-1 rounded text-xs text-muted-foreground hover:bg-secondary hover:text-foreground transition-colors"
          >
            <Trash2 className="w-3 h-3" />
            Clear
          </button>
        </div>
      </div>

      {/* Level filter chips */}
      <div className="flex items-center gap-1 px-4 py-2 border-b border-border shrink-0 overflow-x-auto">
        {FILTERS.map(({ id, label }) => (
          <button
            key={id}
            onClick={() => setFilter(id)}
            className={cn(
              "px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors whitespace-nowrap",
              filter === id
                ? "glass-primary text-white"
                : "bg-secondary text-muted-foreground hover:bg-secondary/80",
            )}
          >
            {label}
            {id !== "all" && counts[id as LogLevel] > 0 && (
              <span className="ml-1 opacity-70">{counts[id as LogLevel]}</span>
            )}
          </button>
        ))}
      </div>

      {/* Log list */}
      <div className="flex-1 overflow-y-auto font-mono text-[11px]">
        {filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full gap-3 text-muted-foreground/50">
            <ScrollText className="w-8 h-8 opacity-30" />
            <p className="text-sm">No log entries{filter !== "all" ? ` for level "${filter}"` : " yet"}</p>
            <p className="text-xs opacity-60">Events from the backend will appear here in real-time.</p>
          </div>
        ) : (
          <>
            {/* newest-first list */}
            {filtered.map((entry) => {
              const meta = LEVEL_META[entry.level];
              return (
                <div
                  key={entry.id}
                  className={cn(
                    "flex gap-2 px-4 py-1.5 hover:bg-secondary/30 transition-colors",
                    meta.row,
                  )}
                >
                  {/* Timestamp */}
                  <span className="shrink-0 text-muted-foreground/50 w-20 pt-0.5">
                    {formatTime(entry.ts)}
                  </span>

                  {/* Level pill */}
                  <span
                    className={cn(
                      "shrink-0 flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium h-fit mt-0.5",
                      meta.pill,
                    )}
                  >
                    {meta.icon}
                    {meta.label}
                  </span>

                  {/* Message */}
                  <span className="flex-1 text-foreground/80 leading-relaxed break-all whitespace-pre-wrap">
                    {entry.message}
                  </span>
                </div>
              );
            })}
            {/* Sentinel for auto-scroll */}
            <div ref={bottomRef} />
          </>
        )}
      </div>
    </div>
  );
}
