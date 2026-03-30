import { useEffect, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Code2,
  File,
  ListTodo,
  Loader2,
  Plus,
  Terminal,
  Trash2,
  X,
} from "lucide-react";
import { useCodingStore } from "@/stores/codingStore";
import { FileExplorer } from "./FileExplorer";
import { CodingChat } from "./CodingChat";
import { TaskPlan } from "./TaskPlan";
import { TerminalOutput } from "./TerminalOutput";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn, formatDate } from "@/lib/utils";
import type { CodingSession } from "@/lib/tauri";

// ── Session sidebar item ──────────────────────────────────────────────────────

function SessionItem({
  session,
  isActive,
  onSelect,
  onDelete,
}: {
  session: CodingSession;
  isActive: boolean;
  onSelect: () => void;
  onDelete: () => void;
}) {
  const modeColors: Record<string, string> = {
    agent: "bg-primary/20 text-primary",
    plan: "bg-violet-500/20 text-violet-300",
    debug: "bg-amber-500/20 text-amber-300",
    ask: "bg-emerald-500/20 text-emerald-300",
  };

  return (
    <div
      className={cn(
        "group px-2 py-2 rounded-lg cursor-pointer transition-colors",
        isActive ? "bg-primary/10 border border-primary/20" : "hover:bg-secondary border border-transparent"
      )}
      onClick={onSelect}
    >
      <div className="flex items-center gap-1.5 min-w-0">
        <span className={cn(
          "text-[9px] font-bold px-1.5 py-0.5 rounded-full uppercase tracking-wider shrink-0",
          modeColors[session.mode] ?? "bg-secondary text-muted-foreground"
        )}>
          {session.mode}
        </span>
        {/* Title truncates — delete button is always reserved so it is never pushed off */}
        <span className="text-xs font-medium truncate min-w-0 flex-1">{session.title.length > 10 ? session.title.slice(0, 10) + "..." : session.title}</span>
        <button
          onClick={(e) => { e.stopPropagation(); onDelete(); }}
          className="shrink-0 p-0.5 rounded text-muted-foreground/40 hover:text-destructive opacity-0 group-hover:opacity-100 transition-all"
          title="Delete session"
        >
          <Trash2 className="w-3 h-3" />
        </button>
      </div>
      <p className="text-[10px] text-muted-foreground/50 mt-0.5 pl-0.5">
        {formatDate(session.created_at)}
      </p>
    </div>
  );
}

// ── File preview panel ────────────────────────────────────────────────────────

function FilePreview() {
  const { openFile, openFileContent } = useCodingStore();

  if (!openFile || !openFileContent) return null;

  return (
    <div className="absolute inset-0 bg-background/95 backdrop-blur-sm z-10 flex flex-col border border-border rounded-lg m-1 overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-border shrink-0">
        <File className="w-3.5 h-3.5 text-muted-foreground" />
        <span className="text-xs font-mono text-muted-foreground flex-1 truncate">{openFile}</span>
        <button
          onClick={() => useCodingStore.getState().openFilePreview("")}
          className="p-0.5 rounded hover:bg-secondary text-muted-foreground hover:text-foreground transition-colors"
          title="Close preview"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>
      <div className="flex-1 overflow-auto">
        <pre className="text-[11px] font-mono p-3 leading-relaxed text-foreground/80 whitespace-pre">
          {openFileContent}
        </pre>
      </div>
    </div>
  );
}


// ── Main view ─────────────────────────────────────────────────────────────────

export function CodingView() {
  const {
    sessions,
    activeSession,
    showPlanPanel,
    showTerminal,
    isRunning,
    currentPlan,
    fetchSessions,
    createSession,
    openSession,
    deleteSession,
    setShowPlanPanel,
    setShowTerminal,
    listenToEvents,
    openFile,
  } = useCodingStore();

  const [leftCollapsed, setLeftCollapsed] = useState(false);

  useEffect(() => {
    fetchSessions();
    const unlistenPromise = listenToEvents();
    return () => {
      unlistenPromise.then((fn) => fn());
    };
  }, []);

  const terminalHeight = showTerminal ? 180 : 0;
  const rightPanelWidth = showPlanPanel ? 240 : 0;
  return (
    <div className="flex h-full overflow-hidden">
      {/* ── Left panel: sessions + file explorer ──────────────────────────── */}
      <div
        className={cn(
          "flex flex-col border-r border-border bg-card/30 shrink-0 transition-all duration-200 overflow-hidden",
          leftCollapsed ? "w-0" : "w-[220px]"
        )}
      >
        {/* Sessions header */}
        <div className="px-2 py-2.5 border-b border-border flex items-center gap-1 shrink-0">
          <Code2 className="w-3.5 h-3.5 text-primary shrink-0" />
          <span className="text-[11px] font-semibold flex-1">Sessions</span>
          <Button
            size="sm"
            variant="ghost"
            className="h-6 w-6 p-0"
            title="New session"
            onClick={createSession}
          >
            <Plus className="w-3.5 h-3.5" />
          </Button>
        </div>

        {/* Sessions list */}
        <ScrollArea className="h-40 shrink-0 border-b border-border">
          <div className="p-1.5 space-y-0.5">
            {sessions.length === 0 ? (
              <p className="text-[11px] text-muted-foreground/40 text-center py-3 px-2">
                No sessions yet
              </p>
            ) : (
              sessions.map((s) => (
                <SessionItem
                  key={s.id}
                  session={s}
                  isActive={activeSession?.id === s.id}
                  onSelect={() => openSession(s.id)}
                  onDelete={() => deleteSession(s.id)}
                />
              ))
            )}
          </div>
        </ScrollArea>

        {/* File explorer fills remaining space */}
        <div className="flex-1 min-h-0 relative">
          <FileExplorer />
          {openFile && <FilePreview />}
        </div>
      </div>

      {/* Toggle button for left panel */}
      <button
        onClick={() => setLeftCollapsed((v) => !v)}
        className="flex items-center justify-center w-4 bg-card/20 border-r border-border hover:bg-secondary transition-colors text-muted-foreground/40 hover:text-muted-foreground shrink-0"
        title={leftCollapsed ? "Show sidebar" : "Hide sidebar"}
      >
        {leftCollapsed ? (
          <ChevronRight className="w-3 h-3" />
        ) : (
          <ChevronLeft className="w-3 h-3" />
        )}
      </button>

      {/* ── Center: chat + terminal ────────────────────────────────────────── */}
      <div className="flex flex-col flex-1 min-w-0 overflow-hidden">
        {/* Chat */}
        <div
          className="flex-1 min-h-0 overflow-hidden"
          style={{ height: `calc(100% - ${terminalHeight}px)` }}
        >
          <CodingChat />
        </div>

        {/* Terminal toggle bar */}
        <div className="border-t border-border shrink-0">
          <button
            onClick={() => setShowTerminal(!showTerminal)}
            className="w-full flex items-center gap-2 px-3 py-1.5 text-[11px] text-muted-foreground hover:bg-secondary transition-colors"
          >
            <Terminal className="w-3 h-3" />
            <span>Terminal</span>
            {isRunning && (
              <span className="flex items-center gap-1 text-primary animate-pulse ml-1">
                <Loader2 className="w-2.5 h-2.5 animate-spin" />
                running
              </span>
            )}
            <ChevronRight
              className={cn(
                "w-3 h-3 ml-auto transition-transform",
                showTerminal && "rotate-90"
              )}
            />
          </button>
          {showTerminal && (
            <div style={{ height: terminalHeight }}>
              <TerminalOutput />
            </div>
          )}
        </div>
      </div>

      {/* ── Right panel: task plan ─────────────────────────────────────────── */}
      <div
        className={cn(
          "flex flex-col border-l border-border bg-card/20 shrink-0 transition-all duration-200 overflow-hidden"
        )}
        style={{ width: rightPanelWidth }}
      >
        <TaskPlan />
      </div>

      {/* Plan panel toggle */}
      <button
        onClick={() => setShowPlanPanel(!showPlanPanel)}
        className="flex flex-col items-center justify-center w-4 bg-card/20 border-l border-border hover:bg-secondary transition-colors text-muted-foreground/40 hover:text-muted-foreground shrink-0 gap-0.5"
        title={showPlanPanel ? "Hide plan panel" : "Show plan panel"}
      >
        {currentPlan && (
          <div className="w-1.5 h-1.5 rounded-full bg-violet-400 mb-1" />
        )}
        <ListTodo className="w-3 h-3" />
        {showPlanPanel ? (
          <ChevronRight className="w-3 h-3" />
        ) : (
          <ChevronLeft className="w-3 h-3" />
        )}
      </button>
    </div>
  );
}
