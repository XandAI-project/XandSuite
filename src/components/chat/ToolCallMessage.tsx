import { useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Wrench,
  CheckCircle2,
  Loader2,
  AlertCircle,
  Terminal,
  Clock,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { ToolStep } from "@/stores/skillsStore";

interface Props {
  steps: ToolStep[];
  /** Whether steps are still accumulating (streaming in progress) */
  isStreaming?: boolean;
}

function prettifyName(name: string): string {
  // "builtin-web-search__web_search" → "web search"
  const bare = name.includes("__") ? name.split("__").pop()! : name;
  return bare.replace(/_/g, " ");
}

// ── Code execution result renderer ───────────────────────────────────────────

interface CodeRunResult {
  stdout?: string;
  stderr?: string;
  exit_code?: number;
  execution_time_ms?: number;
}

function CodeExecutionResult({ raw }: { raw: string }) {
  let result: CodeRunResult = {};
  try {
    result = JSON.parse(raw);
  } catch {
    return (
      <pre className="text-[10px] font-mono whitespace-pre-wrap break-all text-foreground/80 bg-black/20 rounded p-2 max-h-48 overflow-y-auto">
        {raw}
      </pre>
    );
  }

  const { stdout, stderr, exit_code, execution_time_ms } = result;
  const success = exit_code === 0 || exit_code === undefined;

  return (
    <div className="space-y-2">
      {/* Status bar */}
      <div className="flex items-center gap-2 text-[10px]">
        <span
          className={cn(
            "flex items-center gap-1 font-mono px-1.5 py-0.5 rounded border",
            success
              ? "bg-emerald-500/10 border-emerald-500/30 text-emerald-400"
              : "bg-red-500/10 border-red-500/30 text-red-400"
          )}
        >
          exit: {exit_code ?? 0}
        </span>
        {execution_time_ms !== undefined && (
          <span className="flex items-center gap-1 text-muted-foreground">
            <Clock className="w-2.5 h-2.5" />
            {execution_time_ms < 1000
              ? `${execution_time_ms}ms`
              : `${(execution_time_ms / 1000).toFixed(1)}s`}
          </span>
        )}
      </div>

      {/* stdout */}
      {stdout && stdout.trim() && (
        <div>
          <div className="text-[9px] font-semibold uppercase tracking-wider text-emerald-400/70 mb-0.5 flex items-center gap-1">
            <Terminal className="w-2.5 h-2.5" />
            stdout
          </div>
          <pre className="text-[10px] font-mono whitespace-pre-wrap break-words text-emerald-300/90 bg-emerald-950/40 border border-emerald-500/20 rounded p-2 max-h-52 overflow-y-auto leading-relaxed">
            {stdout}
          </pre>
        </div>
      )}

      {/* stderr */}
      {stderr && stderr.trim() && (
        <div>
          <div className="text-[9px] font-semibold uppercase tracking-wider text-red-400/70 mb-0.5 flex items-center gap-1">
            <AlertCircle className="w-2.5 h-2.5" />
            stderr
          </div>
          <pre className="text-[10px] font-mono whitespace-pre-wrap break-words text-red-300/90 bg-red-950/40 border border-red-500/20 rounded p-2 max-h-52 overflow-y-auto leading-relaxed">
            {stderr}
          </pre>
        </div>
      )}

      {/* Empty output */}
      {(!stdout || !stdout.trim()) && (!stderr || !stderr.trim()) && (
        <p className="text-[10px] text-muted-foreground italic">
          (no output)
        </p>
      )}
    </div>
  );
}

// ── Generic tool step card ────────────────────────────────────────────────────

function ToolStepCard({
  step,
  isLast,
  isStreaming,
}: {
  step: ToolStep;
  isLast: boolean;
  isStreaming?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const pending = isLast && isStreaming && step.result === undefined;
  const isError = step.result?.includes('"error"') ?? false;

  const isCodeExec = step.function_name
    ?.toLowerCase()
    .includes("execute_code");

  // Auto-open code execution steps so output is visible immediately
  const [autoOpened, setAutoOpened] = useState(false);
  if (isCodeExec && !pending && step.result !== undefined && !autoOpened) {
    setAutoOpened(true);
    // open on next tick to avoid React state-during-render warning
    setTimeout(() => setOpen(true), 0);
  }

  return (
    <div
      className={cn(
        "rounded-lg border text-xs overflow-hidden transition-colors",
        pending
          ? "border-amber-500/30 bg-amber-500/5"
          : isError
          ? "border-red-500/30 bg-red-500/5"
          : isCodeExec
          ? "border-blue-500/30 bg-blue-500/5"
          : "border-emerald-500/30 bg-emerald-500/5"
      )}
    >
      {/* Header row */}
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-white/5 transition-colors"
      >
        {pending ? (
          <Loader2 className="w-3 h-3 text-amber-400 animate-spin shrink-0" />
        ) : isError ? (
          <AlertCircle className="w-3 h-3 text-red-400 shrink-0" />
        ) : isCodeExec ? (
          <Terminal className="w-3 h-3 text-blue-400 shrink-0" />
        ) : (
          <CheckCircle2 className="w-3 h-3 text-emerald-400 shrink-0" />
        )}
        <Wrench className="w-3 h-3 text-muted-foreground shrink-0" />
        <span
          className={cn(
            "font-medium capitalize",
            pending
              ? "text-amber-300"
              : isError
              ? "text-red-300"
              : isCodeExec
              ? "text-blue-300"
              : "text-emerald-300"
          )}
        >
          {prettifyName(step.function_name)}
        </span>
        <span className="ml-auto text-muted-foreground/50 text-[10px]">
          turn {step.turn + 1}
        </span>
        {open ? (
          <ChevronDown className="w-3 h-3 text-muted-foreground shrink-0" />
        ) : (
          <ChevronRight className="w-3 h-3 text-muted-foreground shrink-0" />
        )}
      </button>

      {/* Expanded details */}
      {open && (
        <div className="border-t border-white/10 px-3 pb-3 pt-2 space-y-2">
          <div>
            <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mb-1">
              Arguments
            </div>
            {isCodeExec && step.arguments?.code ? (
              <pre className="text-[10px] font-mono whitespace-pre-wrap break-all text-blue-200/80 bg-blue-950/30 border border-blue-500/20 rounded p-2 max-h-48 overflow-y-auto">
                {step.arguments.code}
              </pre>
            ) : (
              <pre className="text-[10px] font-mono whitespace-pre-wrap break-all text-foreground/80 bg-black/20 rounded p-2 max-h-32 overflow-y-auto">
                {JSON.stringify(step.arguments, null, 2)}
              </pre>
            )}
          </div>
          {step.result !== undefined && (
            <div>
              <div
                className={cn(
                  "text-[10px] font-semibold uppercase tracking-wider mb-1",
                  isError ? "text-red-400" : isCodeExec ? "text-blue-400" : "text-emerald-400"
                )}
              >
                {isError ? "Error" : "Output"}
              </div>
              {isCodeExec ? (
                <CodeExecutionResult raw={step.result} />
              ) : (
                <pre className="text-[10px] font-mono whitespace-pre-wrap break-all text-foreground/80 bg-black/20 rounded p-2 max-h-48 overflow-y-auto">
                  {(() => {
                    try {
                      return JSON.stringify(JSON.parse(step.result!), null, 2);
                    } catch {
                      return step.result;
                    }
                  })()}
                </pre>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function ToolCallMessage({ steps, isStreaming }: Props) {
  if (steps.length === 0) return null;

  return (
    <div className="space-y-1.5 w-full">
      {steps.map((step, i) => (
        <ToolStepCard
          key={step.tool_call_id}
          step={step}
          isLast={i === steps.length - 1}
          isStreaming={isStreaming}
        />
      ))}
    </div>
  );
}
