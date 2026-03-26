import { memo } from "react";
import { Handle, Position, type NodeProps } from "@reactflow/core";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";

interface NodeData {
  label: string;
  nodeType: string;
  color: string;
  description?: string;
  onDelete?: (id: string) => void;
  /** Set by FlowCanvas during execution */
  _execStatus?: "running" | "done" | "error" | null;
  // Trigger
  trigger_type?: string;
  schedule_cron?: string;
  webhook_path?: string;
  // Prompts / LLM
  prompt?: string;
  temperature?: number;
  max_tokens?: number;
  // Conditional
  condition?: string;
  // Code Exec
  command?: string;
  language?: string;
  timeout_seconds?: number;
  // HTTP API
  url?: string;
  method?: string;
  // Web Search
  query?: string;
  max_results?: number;
  // DB Query
  db_type?: string;
  connection_id?: string;
  // Input / Output
  variable?: string;
  format?: string;
  // Loop
  iterations?: number;
  loop_variable?: string;
  // Merge
  merge_strategy?: string;
}

function getNodeSummary(data: NodeData): string | null {
  switch (data.nodeType) {
    case "trigger": {
      const labels: Record<string, string> = {
        manual: "Manual",
        schedule: `Cron: ${data.schedule_cron || "..."}`,
        webhook: `Webhook: ${data.webhook_path || "..."}`,
        user_request: "User Request",
        event: "Event",
      };
      return labels[data.trigger_type || "manual"] || "Manual";
    }
    case "system_prompt":
      return data.prompt ? truncate(data.prompt, 40) : null;
    case "user_prompt":
    case "template_prompt":
      return data.prompt
        ? `${truncate(data.prompt, 30)} | T:${data.temperature ?? 0.7}`
        : null;
    case "web_search":
      return data.query ? truncate(data.query, 40) : null;
    case "code_exec":
      return data.command
        ? `${data.language || "bash"}: ${truncate(data.command, 30)}`
        : data.language || null;
    case "http_api":
      return data.url ? `${data.method || "GET"} ${truncate(data.url, 28)}` : null;
    case "db_query":
      return data.db_type || null;
    case "conditional":
      return data.condition ? truncate(data.condition, 40) : null;
    case "loop":
      return `${data.iterations ?? 1}x (${data.loop_variable || "i"})`;
    case "merge":
      return data.merge_strategy || "concat";
    case "input":
      return data.variable ? `var: ${data.variable}` : null;
    case "output":
      return data.variable
        ? `var: ${data.variable} (${data.format || "text"})`
        : null;
    default:
      return null;
  }
}

function truncate(s: string, max: number): string {
  return s.length > max ? s.slice(0, max) + "..." : s;
}

export const CustomNode = memo(({ id, data, selected }: NodeProps<NodeData>) => {
  const isInput = data.nodeType === "input";
  const isTrigger = data.nodeType === "trigger";
  const isOutput = data.nodeType === "output";

  const hasInputHandle = !isInput && !isTrigger;
  const hasOutputHandle = !isOutput;

  const summary = getNodeSummary(data);
  const execStatus = data._execStatus;

  return (
    <div
      className={cn(
        "group/node relative rounded-lg border px-3 py-2 min-w-[130px] max-w-[220px] text-xs shadow-md transition-all duration-200",
        data.color || "bg-secondary border-border text-foreground",
        selected && "ring-2 ring-primary ring-offset-1 ring-offset-background",
        execStatus === "running" && "ring-2 ring-yellow-400 ring-offset-1 ring-offset-background shadow-yellow-400/30 shadow-lg",
        execStatus === "done"    && "ring-2 ring-emerald-400 ring-offset-1 ring-offset-background",
        execStatus === "error"   && "ring-2 ring-destructive ring-offset-1 ring-offset-background",
      )}
    >
      {/* Running pulse indicator */}
      {execStatus === "running" && (
        <span className="absolute -top-1.5 -right-1.5 flex h-3 w-3 z-10">
          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-yellow-400 opacity-75" />
          <span className="relative inline-flex rounded-full h-3 w-3 bg-yellow-400" />
        </span>
      )}
      {execStatus === "done" && (
        <span className="absolute -top-1.5 -right-1.5 flex h-3 w-3 rounded-full bg-emerald-400 z-10" />
      )}
      {execStatus === "error" && (
        <span className="absolute -top-1.5 -right-1.5 flex h-3 w-3 rounded-full bg-destructive z-10" />
      )}
      {hasInputHandle && (
        <Handle
          type="target"
          position={Position.Top}
          className="!bg-primary !border-2 !border-background !w-3 !h-3"
        />
      )}

      {/* Delete button — appears on hover */}
      {data.onDelete && (
        <button
          onClick={(e) => { e.stopPropagation(); data.onDelete!(id); }}
          className="absolute -top-2 -right-2 w-4 h-4 rounded-full bg-destructive text-destructive-foreground
                     flex items-center justify-center opacity-0 group-hover/node:opacity-100
                     transition-opacity hover:scale-110 z-10"
          title="Delete node"
        >
          <X className="w-2.5 h-2.5" />
        </button>
      )}

      <div className="font-semibold truncate">{data.label}</div>
      {summary && (
        <div className="mt-1 text-[10px] opacity-70 truncate font-mono">{summary}</div>
      )}
      {data.description && (
        <div className="mt-0.5 text-[9px] opacity-50 truncate italic">{data.description}</div>
      )}
      {hasOutputHandle && (
        <Handle
          type="source"
          position={Position.Bottom}
          className="!bg-primary !border-2 !border-background !w-3 !h-3"
        />
      )}
    </div>
  );
});

CustomNode.displayName = "CustomNode";
