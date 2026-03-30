import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import {
  AlertCircle,
  Bot,
  Brain,
  ChevronDown,
  ChevronRight,
  Eye,
  Loader2,
  Send,
  StopCircle,
  User,
  Wrench,
  Zap,
  CheckCircle,
  XCircle,
  ListTodo,
} from "lucide-react";
import { useCodingStore } from "@/stores/codingStore";
import type { LiveCodingEvent } from "@/stores/codingStore";
import type { CodingMode, CodingMessage } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

// ── Mode selector ─────────────────────────────────────────────────────────────

const MODES: { id: CodingMode; label: string; description: string; color: string }[] = [
  {
    id: "agent",
    label: "Agent",
    description: "Autonomous: reads/writes files, runs commands, iterates",
    color: "text-primary border-primary/50 bg-primary/10",
  },
  {
    id: "plan",
    label: "Plan",
    description: "Analyzes codebase and produces a structured task plan",
    color: "text-violet-400 border-violet-400/50 bg-violet-400/10",
  },
  {
    id: "debug",
    label: "Debug",
    description: "Diagnoses errors, runs tests, applies targeted fixes",
    color: "text-amber-400 border-amber-400/50 bg-amber-400/10",
  },
  {
    id: "ask",
    label: "Ask",
    description: "Read-only Q&A about the codebase",
    color: "text-emerald-400 border-emerald-400/50 bg-emerald-400/10",
  },
];

const MODE_PLACEHOLDERS: Record<CodingMode, string> = {
  agent: "Describe a coding task… e.g. 'Add a dark mode toggle to settings'",
  plan: "Describe a feature to plan… e.g. 'Plan how to add user authentication'",
  debug: "Describe a bug or error… e.g. 'Fix the TypeError in api/auth.ts line 42'",
  ask: "Ask a question… e.g. 'How does the authentication flow work?'",
};

// ── Live event card ───────────────────────────────────────────────────────────

function LiveEventCard({ event }: { event: LiveCodingEvent }) {
  const [open, setOpen] = useState(false);
  const { event_type, payload } = event;

  if (event_type === "started") {
    return (
      <div className="flex items-center gap-2 py-0.5">
        <Zap className="w-3 h-3 text-primary shrink-0" />
        <span className="text-[11px] text-primary font-medium">Session started</span>
      </div>
    );
  }

  if (event_type === "thinking") {
    const text = String(payload.raw ?? payload.thought ?? "");
    return (
      <div className="rounded-md border border-blue-500/20 bg-blue-500/5 overflow-hidden">
        <button
          className="flex w-full items-center gap-2 px-2.5 py-1.5 text-blue-300 hover:bg-blue-500/10 transition-colors"
          onClick={() => setOpen((v) => !v)}
        >
          <Brain className="w-3.5 h-3.5 shrink-0" />
          <span className="text-[11px] font-semibold">Thinking</span>
          {payload.step !== undefined && (
            <Badge variant="secondary" className="text-[10px] ml-1">step {payload.step as number}</Badge>
          )}
          <span className="ml-auto text-[10px] text-blue-400/50 truncate max-w-[50%] hidden sm:block">
            {text.slice(0, 70)}{text.length > 70 ? "…" : ""}
          </span>
          {open ? <ChevronDown className="w-3 h-3 ml-1 shrink-0" /> : <ChevronRight className="w-3 h-3 ml-1 shrink-0" />}
        </button>
        {open && (
          <div className="border-t border-blue-500/20 px-2.5 py-2">
            <p className="text-[11px] text-blue-200/70 whitespace-pre-wrap leading-relaxed">{text}</p>
          </div>
        )}
      </div>
    );
  }

  if (event_type === "action") {
    const toolName = String(payload.tool ?? "unknown");
    const inputJson = payload.input ? JSON.stringify(payload.input, null, 2) : null;
    return (
      <div className="rounded-md border border-amber-500/20 bg-amber-500/5 overflow-hidden">
        <button
          className="flex w-full items-center gap-2 px-2.5 py-1.5 text-amber-300 hover:bg-amber-500/10 transition-colors"
          onClick={() => setOpen((v) => !v)}
        >
          <Wrench className="w-3.5 h-3.5 shrink-0" />
          <span className="text-[11px] font-semibold">Tool call</span>
          <Badge className="bg-amber-500/20 text-amber-300 border-0 text-[10px]">{toolName}</Badge>
          {payload.step !== undefined && (
            <Badge variant="secondary" className="text-[10px]">step {payload.step as number}</Badge>
          )}
          {open ? <ChevronDown className="w-3 h-3 ml-auto shrink-0" /> : <ChevronRight className="w-3 h-3 ml-auto shrink-0" />}
        </button>
        {open && inputJson && (
          <div className="border-t border-amber-500/20 px-2.5 py-2">
            <pre className="text-[11px] font-mono text-amber-100/80 whitespace-pre-wrap break-all bg-black/20 rounded p-2 overflow-x-auto max-h-40">
              {inputJson}
            </pre>
          </div>
        )}
      </div>
    );
  }

  if (event_type === "observation") {
    const isError = Boolean(payload.error);
    const text = String(payload.observation ?? "");
    const toolName = String(payload.tool ?? "");
    return (
      <div className={cn(
        "rounded-md border overflow-hidden",
        isError ? "border-destructive/30 bg-destructive/5" : "border-green-500/20 bg-green-500/5"
      )}>
        <button
          className={cn(
            "flex w-full items-center gap-2 px-2.5 py-1.5 transition-colors",
            isError ? "text-destructive hover:bg-destructive/10" : "text-green-300 hover:bg-green-500/10"
          )}
          onClick={() => setOpen((v) => !v)}
        >
          {isError ? <AlertCircle className="w-3.5 h-3.5 shrink-0" /> : <Eye className="w-3.5 h-3.5 shrink-0" />}
          <span className="text-[11px] font-semibold">{isError ? "Error" : "Result"}</span>
          {toolName && <span className="text-[10px] opacity-60">from {toolName}</span>}
          {open ? <ChevronDown className="w-3 h-3 ml-auto shrink-0" /> : <ChevronRight className="w-3 h-3 ml-auto shrink-0" />}
        </button>
        {open && (
          <div className={cn("border-t px-2.5 py-2", isError ? "border-destructive/20" : "border-green-500/20")}>
            <pre className={cn(
              "text-[11px] font-mono whitespace-pre-wrap break-words max-h-48 overflow-y-auto",
              isError ? "text-destructive/80" : "text-green-200/80"
            )}>
              {text}
            </pre>
          </div>
        )}
      </div>
    );
  }

  if (event_type === "plan_created") {
    return (
      <div className="flex items-center gap-2 py-0.5">
        <ListTodo className="w-3.5 h-3.5 text-violet-400 shrink-0" />
        <span className="text-[11px] text-violet-300 font-medium">Plan created</span>
        <span className="text-[11px] text-muted-foreground">— see Plan panel →</span>
      </div>
    );
  }

  if (event_type === "task_updated") {
    return (
      <div className="flex items-center gap-2 py-0.5">
        <CheckCircle className="w-3 h-3 text-violet-400 shrink-0" />
        <span className="text-[11px] text-muted-foreground">Task updated</span>
      </div>
    );
  }

  if (event_type === "completed") {
    return (
      <div className="flex items-center gap-2 py-0.5">
        <CheckCircle className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
        <span className="text-[11px] text-emerald-300 font-medium">Done</span>
      </div>
    );
  }

  if (event_type === "failed") {
    const reason = String(payload.reason ?? "unknown error");
    return (
      <div className="flex items-center gap-2 py-0.5">
        <XCircle className="w-3.5 h-3.5 text-destructive shrink-0" />
        <span className="text-[11px] text-destructive">{reason}</span>
      </div>
    );
  }

  if (event_type === "cancelled") {
    return (
      <div className="flex items-center gap-2 py-0.5">
        <StopCircle className="w-3 h-3 text-muted-foreground shrink-0" />
        <span className="text-[11px] text-muted-foreground">Cancelled</span>
      </div>
    );
  }

  return null;
}

// ── Message bubble ────────────────────────────────────────────────────────────

function MessageBubble({ message }: { message: CodingMessage }) {
  const isUser = message.role === "user";
  const [showEvents, setShowEvents] = useState(false);

  return (
    <div className={cn("flex gap-2.5", isUser && "flex-row-reverse")}>
      {/* Avatar */}
      <div className={cn(
        "w-7 h-7 rounded-full flex items-center justify-center shrink-0 mt-0.5",
        isUser ? "bg-primary/20" : "bg-secondary"
      )}>
        {isUser
          ? <User className="w-3.5 h-3.5 text-primary" />
          : <Bot className="w-3.5 h-3.5 text-muted-foreground" />
        }
      </div>

      <div className={cn("flex flex-col gap-1 max-w-[85%]", isUser && "items-end")}>
        {/* Content */}
        <div className={cn(
          "rounded-xl px-3 py-2 text-sm",
          isUser
            ? "bg-primary/15 text-foreground"
            : "bg-secondary/60 text-foreground"
        )}>
          {isUser ? (
            <p className="whitespace-pre-wrap leading-relaxed">{message.content}</p>
          ) : (
            <div className="prose prose-sm prose-invert max-w-none">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={{
                  code({ className, children, ...props }) {
                    const match = /language-(\w+)/.exec(className || "");
                    const isBlock = match || String(children).includes("\n");
                    return isBlock ? (
                      <SyntaxHighlighter
                        style={oneDark as Record<string, React.CSSProperties>}
                        language={match?.[1] ?? "text"}
                        PreTag="div"
                        className="rounded-md !text-xs !my-1.5"
                      >
                        {String(children).replace(/\n$/, "")}
                      </SyntaxHighlighter>
                    ) : (
                      <code className="bg-black/30 px-1 py-0.5 rounded text-xs font-mono" {...props}>
                        {children}
                      </code>
                    );
                  },
                }}
              >
                {message.content}
              </ReactMarkdown>
            </div>
          )}
        </div>

        {/* Tool events toggle (assistant messages only) */}
        {!isUser && message.events.length > 0 && (
          <button
            onClick={() => setShowEvents((v) => !v)}
            className="flex items-center gap-1 text-[11px] text-muted-foreground/60 hover:text-muted-foreground transition-colors px-1"
          >
            <Wrench className="w-3 h-3" />
            {message.events.filter(e => e.event_type === "action").length} tool calls
            {showEvents ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
          </button>
        )}
        {showEvents && (
          <div className="w-full space-y-1 pl-1">
            {message.events.map((ev, i) => (
              <LiveEventCard
                key={i}
                event={{ event_type: ev.event_type, payload: ev.payload, timestamp: 0 }}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Live feed ─────────────────────────────────────────────────────────────────

function LiveFeed({ events }: { events: LiveCodingEvent[] }) {
  return (
    <div className="space-y-1.5 py-2">
      <div className="flex items-center gap-2 mb-2">
        <Loader2 className="w-3.5 h-3.5 animate-spin text-primary" />
        <span className="text-xs font-medium text-primary">Running…</span>
      </div>
      {events.map((ev, i) => (
        <LiveEventCard key={i} event={ev} />
      ))}
    </div>
  );
}

// ── Main chat panel ───────────────────────────────────────────────────────────

export function CodingChat() {
  const {
    mode,
    setMode,
    messages,
    liveEvents,
    isRunning,
    sendMessage,
    cancelRun,
    error,
    clearError,
    activeSession,
    createSession,
  } = useCodingStore();

  const [input, setInput] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length, liveEvents.length]);

  const handleSend = async () => {
    const text = input.trim();
    if (!text || isRunning) return;
    setInput("");
    await sendMessage(text);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const activeMode = MODES.find((m) => m.id === mode) ?? MODES[0];

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* Mode selector */}
      <div className="px-4 py-2.5 border-b border-border flex items-center gap-1.5 shrink-0 flex-wrap">
        {MODES.map((m) => (
          <button
            key={m.id}
            title={m.description}
            onClick={() => setMode(m.id)}
            className={cn(
              "px-3 py-1 rounded-full text-xs font-medium border transition-colors",
              mode === m.id
                ? m.color
                : "text-muted-foreground border-transparent hover:border-border hover:text-foreground"
            )}
          >
            {m.label}
          </button>
        ))}
        <div className="ml-auto flex items-center gap-2">
          {isRunning && (
            <Button
              size="sm"
              variant="outline"
              className="gap-1.5 h-7 text-xs border-destructive/50 text-destructive hover:bg-destructive/10"
              onClick={cancelRun}
            >
              <StopCircle className="w-3 h-3" />
              Stop
            </Button>
          )}
          <span className="text-[11px] text-muted-foreground/50 hidden lg:block">
            {activeMode.description}
          </span>
        </div>
      </div>

      {/* Messages area */}
      <ScrollArea className="flex-1 min-h-0" ref={scrollRef}>
        <div className="px-4 py-4 space-y-4">
          {messages.length === 0 && !isRunning && (
            <div className="flex flex-col items-center justify-center pt-12 text-center gap-3">
              <Bot className="w-12 h-12 text-muted-foreground/20" />
              <div>
                <p className="text-sm text-muted-foreground font-medium">
                  {activeMode.label} mode
                </p>
                <p className="text-xs text-muted-foreground/60 mt-1">
                  {activeMode.description}
                </p>
              </div>
              {!activeSession && (
                <button
                  onClick={createSession}
                  className="text-xs text-primary/70 hover:text-primary underline underline-offset-2 transition-colors"
                >
                  Start a new session
                </button>
              )}
            </div>
          )}

          {messages.map((msg) => (
            <MessageBubble key={msg.id} message={msg} />
          ))}

          {isRunning && <LiveFeed events={liveEvents} />}

          <div ref={endRef} />
        </div>
      </ScrollArea>

      {/* Error bar */}
      {error && (
        <div className="mx-4 mb-2 flex items-start gap-2 p-2.5 rounded-lg bg-destructive/10 border border-destructive/20 text-xs text-destructive shrink-0">
          <AlertCircle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
          <span className="flex-1">{error}</span>
          <button onClick={clearError} className="shrink-0 hover:opacity-70">✕</button>
        </div>
      )}

      {/* Input bar */}
      <div className="px-4 pb-4 pt-2 shrink-0">
        <div className="flex gap-2 items-end">
          <Textarea
            placeholder={MODE_PLACEHOLDERS[mode]}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            disabled={isRunning}
            rows={3}
            className="resize-none flex-1 text-sm"
          />
          <Button
            onClick={handleSend}
            disabled={!input.trim() || isRunning}
            size="sm"
            className="h-9 px-3 shrink-0"
          >
            {isRunning ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Send className="w-4 h-4" />
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
