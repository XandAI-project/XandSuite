import { CheckCircle, Circle, Clock, ListTodo, Loader2, Play, XCircle } from "lucide-react";
import { useCodingStore } from "@/stores/codingStore";
import type { CodingPlanTask } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

// ── Task status icon ──────────────────────────────────────────────────────────

function TaskIcon({ status }: { status: CodingPlanTask["status"] }) {
  switch (status) {
    case "completed":
      return <CheckCircle className="w-4 h-4 text-emerald-400 shrink-0" />;
    case "in_progress":
      return <Loader2 className="w-4 h-4 text-primary animate-spin shrink-0" />;
    case "failed":
      return <XCircle className="w-4 h-4 text-destructive shrink-0" />;
    default:
      return <Circle className="w-4 h-4 text-muted-foreground/40 shrink-0" />;
  }
}

// ── Single task row ───────────────────────────────────────────────────────────

function TaskRow({ task, index }: { task: CodingPlanTask; index: number }) {
  return (
    <div
      className={cn(
        "flex items-start gap-2.5 px-3 py-2.5 rounded-lg border transition-colors",
        task.status === "completed" && "border-emerald-500/20 bg-emerald-500/5",
        task.status === "in_progress" && "border-primary/30 bg-primary/5",
        task.status === "failed" && "border-destructive/30 bg-destructive/5",
        task.status === "pending" && "border-border bg-transparent"
      )}
    >
      <div className="mt-0.5">
        <TaskIcon status={task.status} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-[10px] font-mono text-muted-foreground/40 shrink-0">
            {String(index + 1).padStart(2, "0")}
          </span>
          <span
            className={cn(
              "text-xs font-medium leading-snug",
              task.status === "completed" && "line-through text-muted-foreground/50",
              task.status === "in_progress" && "text-foreground",
              task.status === "failed" && "text-destructive",
              task.status === "pending" && "text-muted-foreground"
            )}
          >
            {task.title}
          </span>
        </div>
        {task.description && (
          <p className="text-[11px] text-muted-foreground/60 mt-1 leading-snug">
            {task.description}
          </p>
        )}
        {task.note && (
          <p
            className={cn(
              "text-[11px] mt-1 italic",
              task.status === "failed" ? "text-destructive/70" : "text-muted-foreground/50"
            )}
          >
            {task.note}
          </p>
        )}
      </div>
    </div>
  );
}

// ── Progress bar ──────────────────────────────────────────────────────────────

function PlanProgress({ tasks }: { tasks: CodingPlanTask[] }) {
  const total = tasks.length;
  if (total === 0) return null;

  const done = tasks.filter((t) => t.status === "completed").length;
  const failed = tasks.filter((t) => t.status === "failed").length;
  const pct = Math.round((done / total) * 100);

  return (
    <div className="px-3 py-2 border-b border-border">
      <div className="flex items-center justify-between mb-1.5">
        <span className="text-[11px] text-muted-foreground">
          {done}/{total} completed
        </span>
        <span className="text-[11px] font-medium text-primary">{pct}%</span>
      </div>
      <div className="w-full h-1.5 bg-secondary rounded-full overflow-hidden">
        <div
          className="h-full bg-emerald-500 rounded-full transition-all duration-500"
          style={{ width: `${pct}%` }}
        />
      </div>
      {failed > 0 && (
        <p className="text-[10px] text-destructive/70 mt-1">{failed} task(s) failed</p>
      )}
    </div>
  );
}

// ── Empty state ───────────────────────────────────────────────────────────────

function EmptyPlan({ isRunning }: { isRunning: boolean }) {
  return (
    <div className="flex flex-col items-center justify-center h-full gap-3 px-4 py-8 text-center">
      {isRunning ? (
        <>
          <Loader2 className="w-8 h-8 text-muted-foreground/20 animate-spin" />
          <p className="text-xs text-muted-foreground/50">
            Waiting for agent to create a plan…
          </p>
        </>
      ) : (
        <>
          <ListTodo className="w-8 h-8 text-muted-foreground/20" />
          <p className="text-xs text-muted-foreground/50 leading-relaxed">
            Plans are created automatically when you run in <strong className="text-muted-foreground">Plan</strong> or <strong className="text-muted-foreground">Agent</strong> mode.
          </p>
        </>
      )}
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export function TaskPlan() {
  const { currentPlan, isRunning, executePlan } = useCodingStore();

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* Header */}
      <div className="px-3 py-2.5 border-b border-border flex items-center gap-2 shrink-0">
        <ListTodo className="w-3.5 h-3.5 text-violet-400 shrink-0" />
        <span className="text-xs font-semibold">
          {currentPlan ? currentPlan.title : "Task Plan"}
        </span>
        {isRunning && !currentPlan && (
          <Loader2 className="w-3 h-3 animate-spin text-muted-foreground/50 ml-auto" />
        )}
        {currentPlan && (
          <span className="ml-auto text-[10px] text-muted-foreground/50">
            {currentPlan.tasks.length} tasks
          </span>
        )}
      </div>

      {/* Progress */}
      {currentPlan && <PlanProgress tasks={currentPlan.tasks} />}

      {/* Execute Plan button — shown when plan exists, not running, and has pending tasks */}
      {currentPlan &&
        !isRunning &&
        currentPlan.tasks.some((t) => t.status === "pending") && (
          <div className="px-3 py-2 border-b border-border shrink-0">
            <Button
              onClick={executePlan}
              size="sm"
              className="w-full gap-2"
            >
              <Play className="w-3.5 h-3.5" />
              Execute Plan
            </Button>
          </div>
        )}

      {/* Tasks */}
      <ScrollArea className="flex-1 min-h-0">
        {currentPlan && currentPlan.tasks.length > 0 ? (
          <div className="p-2 space-y-1.5">
            {currentPlan.tasks.map((task, i) => (
              <TaskRow key={task.id} task={task} index={i} />
            ))}
          </div>
        ) : (
          <EmptyPlan isRunning={isRunning} />
        )}
      </ScrollArea>

      {/* Status footer */}
      {currentPlan && (
        <div className="px-3 py-2 border-t border-border shrink-0">
          <div className="flex items-center gap-1.5">
            <Clock className="w-3 h-3 text-muted-foreground/40" />
            <span className="text-[10px] text-muted-foreground/40">
              {new Date(currentPlan.updated_at).toLocaleTimeString()}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
