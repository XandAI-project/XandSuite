import { useEffect, useRef, useState } from "react";
import {
  Bot, Play, ChevronDown, ChevronRight, Brain, Wrench, Eye,
  CheckCircle, XCircle, Loader2, Trash2, StopCircle, Zap, AlertCircle,
  FolderOpen, Download, FileText,
} from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { useAgentStore } from "@/stores/agentStore";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn, formatDate } from "@/lib/utils";
import { invoke } from "@/lib/tauri";
import type { AgentEvent, AgentTask } from "@/lib/tauri";

const EXAMPLE_TASKS = [
  {
    icon: "🔍",
    label: "AI News Summary",
    prompt: "Search the web for the latest news about artificial intelligence published this week and write a 3-paragraph summary of the most important developments.",
  },
  {
    icon: "🐍",
    label: "Python Script",
    prompt: "Write a Python script that reads a CSV file named 'data.csv', calculates basic statistics (mean, median, min, max) for each numeric column, and prints a formatted report.",
  },
  {
    icon: "⚖️",
    label: "Framework Comparison",
    prompt: "Search the web and compare three popular backend frameworks: FastAPI, Express.js, and Go Gin. Create a comparison table covering performance, ease of use, ecosystem, and best use cases.",
  },
  {
    icon: "🌐",
    label: "Fetch Public API",
    prompt: "Fetch data from https://api.publicapis.org/entries?category=Animals&https=true and summarize the top 5 public APIs listed, including their descriptions and authentication requirements.",
  },
  {
    icon: "📝",
    label: "Write Blog Post",
    prompt: "Write a 400-word blog post titled 'Why Local AI Models Are the Future of Privacy' aimed at developers. Include an introduction, 3 key points with examples, and a conclusion with a call to action.",
  },
  {
    icon: "🧮",
    label: "Math Explanation",
    prompt: "Explain the Fibonacci sequence, show the first 15 numbers, provide the closed-form formula (Binet's formula), and write a Python function to compute it efficiently using memoization.",
  },
];

export function AgentTaskView() {
  // Selecting the exact fields used here (with useShallow) instead of the
  // whole store — a plain `useAgentStore()` call re-renders on every `set()`
  // in the store, including fields this component never reads (e.g. from
  // `isRunning`/`clearError` callers elsewhere).
  const {
    tasks,
    activeTask,
    runningTaskIds,
    liveEventsByTask,
    fetchTasks,
    runTask,
    deleteTask,
    cancelTask,
    setActiveTask,
    listenToEvents,
    error,
  } = useAgentStore(
    useShallow((s) => ({
      tasks: s.tasks,
      activeTask: s.activeTask,
      runningTaskIds: s.runningTaskIds,
      liveEventsByTask: s.liveEventsByTask,
      fetchTasks: s.fetchTasks,
      runTask: s.runTask,
      deleteTask: s.deleteTask,
      cancelTask: s.cancelTask,
      setActiveTask: s.setActiveTask,
      listenToEvents: s.listenToEvents,
      error: s.error,
    }))
  );

  const [taskDescription, setTaskDescription] = useState("");
  const eventsEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    fetchTasks();
    const unlistenPromise = listenToEvents();
    return () => {
      unlistenPromise.then((fn) => fn());
    };
  }, []);

  // Auto-scroll live events
  const activeLiveEvents = activeTask
    ? (liveEventsByTask[activeTask.id] ?? [])
    : [];
  useEffect(() => {
    eventsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [activeLiveEvents.length]);

  const handleRun = async () => {
    const text = taskDescription.trim();
    if (!text) return;
    setTaskDescription("");
    await runTask(text);
  };

  const anyRunning = runningTaskIds.size > 0;
  const activeIsRunning = activeTask
    ? runningTaskIds.has(activeTask.id)
    : false;

  return (
    <div className="flex h-full">
      {/* ── Sidebar ────────────────────────────────────────────────────── */}
      <div className="w-64 flex flex-col border-r border-border bg-card/50 shrink-0">
        <div className="p-3 border-b border-border flex items-center justify-between">
          <span className="text-sm font-semibold">Agent Tasks</span>
          {anyRunning && (
            <span className="flex items-center gap-1 text-[10px] text-primary animate-pulse">
              <Loader2 className="w-3 h-3 animate-spin" />
              {runningTaskIds.size} running
            </span>
          )}
        </div>
        <ScrollArea className="flex-1">
          <div className="p-2 space-y-1">
            {tasks.map((task) => (
              <SidebarItem
                key={task.id}
                task={task}
                isActive={activeTask?.id === task.id}
                isRunning={runningTaskIds.has(task.id)}
                onClick={() => setActiveTask(task)}
                onDelete={() => deleteTask(task.id)}
                onCancel={() => cancelTask(task.id)}
              />
            ))}
            {tasks.length === 0 && (
              <p className="text-[11px] text-muted-foreground px-2 py-3 text-center">
                No tasks yet
              </p>
            )}
          </div>
        </ScrollArea>
      </div>

      {/* ── Main panel ─────────────────────────────────────────────────── */}
      <div className="flex-1 flex flex-col h-full min-w-0">
        <div className="px-6 py-4 border-b border-border flex items-center justify-between">
          <div>
            <h1 className="text-xl font-semibold">Agentic Tasks</h1>
            <p className="text-sm text-muted-foreground mt-0.5">
              ReAct-based autonomous agent with tool use
            </p>
          </div>
          {activeIsRunning && (
            <Button
              variant="outline"
              size="sm"
              className="gap-1.5 border-destructive/50 text-destructive hover:bg-destructive/10"
              onClick={() => activeTask && cancelTask(activeTask.id)}
            >
              <StopCircle className="w-3.5 h-3.5" />
              Cancel
            </Button>
          )}
        </div>

        {/* Task input — always enabled, user can queue tasks */}
        <div className="px-6 py-4 border-b border-border">
          {error && (
            <div className="mb-3 flex items-start gap-2 p-3 rounded-lg bg-destructive/10 border border-destructive/20 text-sm text-destructive">
              <AlertCircle className="w-4 h-4 shrink-0 mt-0.5" />
              {error}
            </div>
          )}
          <Textarea
            placeholder="Describe a task… e.g. 'Search the web for the latest AI news and summarize the top 3 results'"
            value={taskDescription}
            onChange={(e) => setTaskDescription(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleRun();
              }
            }}
            className="mb-3 resize-none"
            rows={3}
          />
          <div className="flex items-center gap-2 flex-wrap mb-3">
            <span className="text-[11px] text-muted-foreground shrink-0">Examples:</span>
            {EXAMPLE_TASKS.map((ex) => (
              <button
                key={ex.label}
                title={ex.prompt}
                onClick={() => setTaskDescription(ex.prompt)}
                className="flex items-center gap-1 px-2 py-1 rounded-full text-[11px] border border-border bg-secondary hover:bg-secondary/80 hover:border-primary/40 text-muted-foreground hover:text-foreground transition-colors truncate max-w-[200px]"
              >
                <span>{ex.icon}</span>
                <span className="truncate">{ex.label}</span>
              </button>
            ))}
          </div>
          <Button onClick={handleRun} disabled={!taskDescription.trim()}>
            <Play className="w-4 h-4 mr-2" />
            Run Agent Task
          </Button>
        </div>

        {/* Live events or task detail */}
        <ScrollArea className="flex-1 px-6 py-4">
          {activeTask ? (
            activeIsRunning ? (
              <LiveFeed
                events={activeLiveEvents}
                endRef={eventsEndRef}
              />
            ) : (
              <TaskDetail task={activeTask} />
            )
          ) : (
            <EmptyState />
          )}
        </ScrollArea>
      </div>
    </div>
  );
}

// ── Sidebar item ────────────────────────────────────────────────────────────

function SidebarItem({
  task, isActive, isRunning, onClick, onDelete, onCancel,
}: {
  task: AgentTask;
  isActive: boolean;
  isRunning: boolean;
  onClick: () => void;
  onDelete: () => void;
  onCancel: () => void;
}) {
  return (
    <div
      className={cn(
        "p-2 rounded-lg cursor-pointer transition-colors",
        isActive ? "bg-primary/10" : "hover:bg-secondary"
      )}
      onClick={onClick}
    >
      <div className="flex items-center gap-1.5">
        <TaskStatusIcon status={task.status} />
        <span className="text-xs font-medium truncate min-w-0">{task.title}</span>
      </div>
      <div className="flex items-center gap-1 mt-1">
        <button
          title="Delete task"
          onClick={(e) => { e.stopPropagation(); onDelete(); }}
          className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors border border-transparent hover:border-destructive/20"
        >
          <Trash2 className="w-3 h-3" />
          Delete
        </button>
        {isRunning && (
          <button
            title="Cancel task"
            onClick={(e) => { e.stopPropagation(); onCancel(); }}
            className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] text-muted-foreground hover:text-amber-400 hover:bg-amber-400/10 transition-colors border border-transparent hover:border-amber-400/20"
          >
            <StopCircle className="w-3 h-3" />
            Cancel
          </button>
        )}
      </div>
      <div className="text-[10px] text-muted-foreground mt-0.5">
        {formatDate(task.created_at)}
      </div>
    </div>
  );
}

// ── Status icon ─────────────────────────────────────────────────────────────

function TaskStatusIcon({ status }: { status: AgentTask["status"] }) {
  switch (status) {
    case "running":
      return <Loader2 className="w-3 h-3 text-primary animate-spin shrink-0" />;
    case "completed":
      return <CheckCircle className="w-3 h-3 text-emerald-400 shrink-0" />;
    case "failed":
      return <XCircle className="w-3 h-3 text-destructive shrink-0" />;
    case "cancelled":
      return <StopCircle className="w-3 h-3 text-muted-foreground shrink-0" />;
    default:
      return <Bot className="w-3 h-3 text-muted-foreground shrink-0" />;
  }
}

// ── Live feed (while running) ────────────────────────────────────────────────

function LiveFeed({
  events,
  endRef,
}: {
  events: AgentEvent[];
  endRef: React.RefObject<HTMLDivElement | null>;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2 mb-3 text-sm font-medium text-primary">
        <Loader2 className="w-4 h-4 animate-spin" />
        Agent is running…
      </div>
      {events.map((event, i) => (
        <EventCard key={i} event={event} />
      ))}
      <div ref={endRef} />
    </div>
  );
}

// ── Rich EventCard ────────────────────────────────────────────────────────────

function EventCard({ event }: { event: AgentEvent }) {
  const [open, setOpen] = useState(false);
  const { event_type, payload } = event;
  const step = payload.step as number | undefined;

  if (event_type === "started") {
    return (
      <div className="flex items-center gap-2 py-1">
        <Zap className="w-3.5 h-3.5 text-primary shrink-0" />
        <span className="text-xs font-semibold text-primary">Task started</span>
        {payload.task != null && (
          <span className="text-xs text-muted-foreground truncate">
            — {String(payload.task)}
          </span>
        )}
      </div>
    );
  }

  if (event_type === "llm_generating") {
    return (
      <div className="flex items-center gap-2 py-1">
        <Loader2 className="w-3 h-3 text-muted-foreground animate-spin shrink-0" />
        <span className="text-xs text-muted-foreground italic">
          Generating step {step}…
        </span>
      </div>
    );
  }

  if (event_type === "thought") {
    const text = String(payload.raw ?? payload.thought ?? "");
    return (
      <div className="rounded-lg border border-blue-500/20 bg-blue-500/5 overflow-hidden">
        <button
          className="flex w-full items-center gap-2 px-3 py-2 text-blue-300 hover:bg-blue-500/10 transition-colors"
          onClick={() => setOpen((v) => !v)}
        >
          <Brain className="w-3.5 h-3.5 shrink-0" />
          <span className="text-xs font-semibold">Thought</span>
          {step !== undefined && (
            <Badge variant="secondary" className="text-[10px] ml-1">
              step {step}
            </Badge>
          )}
          <span className="ml-auto text-[11px] text-blue-400/60 truncate max-w-[55%] hidden sm:block">
            {text.slice(0, 80)}{text.length > 80 ? "…" : ""}
          </span>
          {open ? (
            <ChevronDown className="w-3 h-3 shrink-0" />
          ) : (
            <ChevronRight className="w-3 h-3 shrink-0" />
          )}
        </button>
        {open && (
          <div className="border-t border-blue-500/20 px-3 py-2">
            <p className="text-xs text-blue-200/70 whitespace-pre-wrap leading-relaxed">
              {text || <em className="opacity-50">No thought text</em>}
            </p>
          </div>
        )}
      </div>
    );
  }

  if (event_type === "action") {
    const toolName = String(payload.tool ?? "unknown");
    const thought = String(payload.thought ?? "");
    const inputJson = payload.input
      ? JSON.stringify(payload.input, null, 2)
      : null;
    return (
      <div className="rounded-lg border border-amber-500/20 bg-amber-500/5 overflow-hidden">
        <button
          className="flex w-full items-center gap-2 px-3 py-2 text-amber-300 hover:bg-amber-500/10 transition-colors"
          onClick={() => setOpen((v) => !v)}
        >
          <Wrench className="w-3.5 h-3.5 shrink-0" />
          <span className="text-xs font-semibold">Action</span>
          <Badge className="bg-amber-500/20 text-amber-300 border-0 text-[10px]">
            {toolName}
          </Badge>
          {step !== undefined && (
            <Badge variant="secondary" className="text-[10px]">
              step {step}
            </Badge>
          )}
          {open ? (
            <ChevronDown className="w-3 h-3 ml-auto shrink-0" />
          ) : (
            <ChevronRight className="w-3 h-3 ml-auto shrink-0" />
          )}
        </button>
        {open && (
          <div className="border-t border-amber-500/20 px-3 py-2 space-y-2">
            {thought && (
              <p className="text-[11px] text-amber-200/60 italic">
                Thought: {thought}
              </p>
            )}
            {inputJson && (
              <pre className="text-xs text-amber-100/80 bg-black/20 rounded p-2 overflow-x-auto whitespace-pre-wrap break-all font-mono">
                {inputJson}
              </pre>
            )}
          </div>
        )}
      </div>
    );
  }

  if (event_type === "observation") {
    const isError = Boolean(payload.error);
    const text = String(payload.observation ?? "");
    return (
      <div
        className={cn(
          "rounded-lg border overflow-hidden",
          isError
            ? "border-destructive/30 bg-destructive/5"
            : "border-green-500/20 bg-green-500/5"
        )}
      >
        <button
          className={cn(
            "flex w-full items-center gap-2 px-3 py-2 transition-colors",
            isError
              ? "text-destructive hover:bg-destructive/10"
              : "text-green-300 hover:bg-green-500/10"
          )}
          onClick={() => setOpen((v) => !v)}
        >
          {isError ? (
            <AlertCircle className="w-3.5 h-3.5 shrink-0" />
          ) : (
            <Eye className="w-3.5 h-3.5 shrink-0" />
          )}
          <span className="text-xs font-semibold">
            {isError ? "Error" : "Observation"}
          </span>
          {payload.tool != null && (
            <span className="text-[11px] opacity-60">from {String(payload.tool)}</span>
          )}
          {step !== undefined && (
            <Badge variant="secondary" className="text-[10px]">
              step {step}
            </Badge>
          )}
          {open ? (
            <ChevronDown className="w-3 h-3 ml-auto shrink-0" />
          ) : (
            <ChevronRight className="w-3 h-3 ml-auto shrink-0" />
          )}
        </button>
        {open && (
          <div
            className={cn(
              "border-t px-3 py-2",
              isError ? "border-destructive/20" : "border-green-500/20"
            )}
          >
            <pre
              className={cn(
                "text-xs whitespace-pre-wrap break-words max-h-48 overflow-y-auto font-mono",
                isError ? "text-destructive/80" : "text-green-200/80"
              )}
            >
              {text}
            </pre>
          </div>
        )}
      </div>
    );
  }

  if (event_type === "completed") {
    return (
      <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3">
        <div className="flex items-center gap-2 mb-1.5">
          <CheckCircle className="w-4 h-4 text-emerald-400 shrink-0" />
          <span className="text-sm font-semibold text-emerald-300">
            Task completed
          </span>
        </div>
        {payload.result != null && (
          <p className="text-xs text-emerald-200/80 whitespace-pre-wrap">
            {String(payload.result)}
          </p>
        )}
      </div>
    );
  }

  if (event_type === "failed") {
    const reason = String(payload.reason ?? "unknown error");
    const reasonLabel: Record<string, string> = {
      timeout: "Timed out",
      max_iterations_exceeded: "Max iterations reached",
      repetition_loop: "Stuck in a loop",
      no_llm_response: "No model response — is a model loaded?",
    };
    return (
      <div className="rounded-lg border border-destructive/30 bg-destructive/10 p-3">
        <div className="flex items-center gap-2 mb-1">
          <XCircle className="w-4 h-4 text-destructive shrink-0" />
          <span className="text-sm font-semibold text-destructive">
            Task failed
          </span>
        </div>
        <p className="text-xs text-destructive/80">
          {reasonLabel[reason] ?? reason}
        </p>
        {payload.tool != null && (
          <p className="text-xs text-muted-foreground mt-1">
            Tool: {String(payload.tool)}
          </p>
        )}
      </div>
    );
  }

  if (event_type === "cancelled") {
    return (
      <div className="flex items-center gap-2 py-1 text-muted-foreground">
        <StopCircle className="w-3.5 h-3.5 shrink-0" />
        <span className="text-xs">Task cancelled</span>
      </div>
    );
  }

  // Fallback for unknown event types
  return (
    <div className="flex items-center gap-2 text-xs text-muted-foreground py-1">
      <Bot className="w-3.5 h-3.5 shrink-0" />
      <span className="capitalize">{event_type}</span>
    </div>
  );
}

// ── Completed task detail ────────────────────────────────────────────────────

function TaskDetail({ task }: { task: AgentTask }) {
  const [expandedSteps, setExpandedSteps] = useState<Set<number>>(
    new Set([0])
  );

  const toggleStep = (i: number) => {
    setExpandedSteps((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };

  return (
    <div className="space-y-4">
      <div>
        <div className="flex items-center gap-2 mb-1">
          <TaskStatusIcon status={task.status} />
          <h3 className="font-semibold">{task.title}</h3>
          <Badge variant="secondary" className="text-[10px] capitalize">
            {task.status}
          </Badge>
        </div>
        <p className="text-sm text-muted-foreground">{task.description}</p>
      </div>

      {task.result && (
        <div
          className={cn(
            "p-4 rounded-lg border",
            task.status === "completed"
              ? "bg-emerald-500/10 border-emerald-500/20"
              : "bg-destructive/10 border-destructive/20"
          )}
        >
          <div
            className={cn(
              "text-xs font-semibold mb-2",
              task.status === "completed"
                ? "text-emerald-400"
                : "text-destructive"
            )}
          >
            {task.status === "completed" ? "Result" : "Failure reason"}
          </div>
          <p className="text-sm whitespace-pre-wrap">{task.result}</p>
        </div>
      )}

      {task.steps.length > 0 && (
        <div>
          <div className="text-sm font-semibold mb-2">
            Steps ({task.steps.length})
          </div>
          <div className="space-y-2">
            {task.steps.map((step, i) => (
              <div
                key={i}
                className="border border-border rounded-lg overflow-hidden"
              >
                <button
                  className="w-full flex items-center gap-2 p-3 hover:bg-secondary/50 text-left"
                  onClick={() => toggleStep(i)}
                >
                  {expandedSteps.has(i) ? (
                    <ChevronDown className="w-3.5 h-3.5 shrink-0" />
                  ) : (
                    <ChevronRight className="w-3.5 h-3.5 shrink-0" />
                  )}
                  <span className="text-xs font-medium">
                    Step {step.step_number}
                  </span>
                  {step.action && (
                    <Badge variant="secondary" className="text-[10px]">
                      {step.action.tool_name}
                    </Badge>
                  )}
                  {step.action?.error && (
                    <Badge
                      variant="destructive"
                      className="text-[10px]"
                    >
                      error
                    </Badge>
                  )}
                </button>
                {expandedSteps.has(i) && (
                  <div className="border-t border-border p-3 space-y-2">
                    {step.thought && (
                      <StepSection
                        icon={Brain}
                        label="Thought"
                        content={step.thought}
                        color="blue"
                      />
                    )}
                    {step.action && (
                      <StepSection
                        icon={Wrench}
                        label={`Action: ${step.action.tool_name}`}
                        content={JSON.stringify(step.action.input, null, 2)}
                        color="amber"
                        isCode
                      />
                    )}
                    {step.action?.error && (
                      <StepSection
                        icon={AlertCircle}
                        label="Error"
                        content={step.action.error}
                        color="red"
                      />
                    )}
                    {step.observation && (
                      <StepSection
                        icon={Eye}
                        label="Observation"
                        content={step.observation}
                        color="green"
                      />
                    )}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Workspace files — always show if any files were created */}
      <WorkspaceFiles taskId={task.id} />
    </div>
  );
}

function StepSection({
  icon: Icon,
  label,
  content,
  color,
  isCode,
}: {
  icon: typeof Bot;
  label: string;
  content: string;
  color: string;
  isCode?: boolean;
}) {
  const colorMap: Record<string, string> = {
    blue: "text-blue-400 bg-blue-500/10 border-blue-500/20",
    amber: "text-amber-400 bg-amber-500/10 border-amber-500/20",
    green: "text-green-400 bg-green-500/10 border-green-500/20",
    red: "text-destructive bg-destructive/10 border-destructive/20",
  };
  return (
    <div className={cn("rounded-md p-2 border", colorMap[color] ?? colorMap.blue)}>
      <div className="flex items-center gap-1.5 mb-1">
        <Icon className="w-3 h-3" />
        <span className="text-[10px] font-semibold uppercase tracking-wider">
          {label}
        </span>
      </div>
      {isCode ? (
        <pre className="text-xs whitespace-pre-wrap break-all font-mono max-h-64 overflow-y-auto">
          {content}
        </pre>
      ) : (
        <p className="text-xs whitespace-pre-wrap">{content}</p>
      )}
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-col items-center justify-center h-full text-center pt-12">
      <Bot className="w-12 h-12 text-muted-foreground mb-3 opacity-40" />
      <p className="text-muted-foreground text-sm">
        Enter a task above to run the agent,
        <br />
        or select a past task from the sidebar.
      </p>
    </div>
  );
}

// ── Workspace files panel ────────────────────────────────────────────────────

interface TaskFile { name: string; size_bytes: number }

function WorkspaceFiles({ taskId }: { taskId: string }) {
  const [files, setFiles] = useState<TaskFile[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    invoke<TaskFile[]>("list_task_files", { taskId })
      .then(setFiles)
      .catch(() => setFiles([]))
      .finally(() => setLoading(false));
  }, [taskId]);

  if (loading) return null;
  if (files.length === 0) return null;

  const openFolder = () => invoke("open_task_workspace", { taskId }).catch(console.error);

  const downloadFile = async (filename: string) => {
    try {
      const content = await invoke<string>("read_task_file", { taskId, filename });
      const blob = new Blob([content], { type: "text/plain" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = filename;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      console.error("Failed to download file:", e);
    }
  };

  return (
    <div className="mt-4">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-1.5 text-sm font-semibold">
          <FileText className="w-3.5 h-3.5 text-primary" />
          Created Files ({files.length})
        </div>
        <button
          onClick={openFolder}
          className="flex items-center gap-1 text-[11px] px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:border-primary/40 transition-colors"
        >
          <FolderOpen className="w-3 h-3" />
          Open Folder
        </button>
      </div>
      <div className="space-y-1">
        {files.map((file) => (
          <div
            key={file.name}
            className="flex items-center gap-2 px-3 py-2 rounded-lg border border-border bg-secondary/30 text-xs"
          >
            <FileText className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
            <span className="flex-1 truncate font-mono">{file.name}</span>
            <span className="text-muted-foreground/60 shrink-0">
              {file.size_bytes < 1024
                ? `${file.size_bytes} B`
                : `${(file.size_bytes / 1024).toFixed(1)} KB`}
            </span>
            <button
              onClick={() => downloadFile(file.name)}
              className="flex items-center gap-1 px-2 py-0.5 rounded text-[10px] border border-primary/30 text-primary hover:bg-primary/10 transition-colors shrink-0"
            >
              <Download className="w-3 h-3" />
              Download
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
