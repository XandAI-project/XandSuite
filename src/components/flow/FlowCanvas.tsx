import { useCallback, useEffect, useRef, useState, useMemo } from "react";
import {
  ReactFlow,
  addEdge,
  useNodesState,
  useEdgesState,
  type Connection,
  type NodeTypes,
  type Node,
} from "@reactflow/core";
import { Background, BackgroundVariant } from "@reactflow/background";
import { Controls } from "@reactflow/controls";
import { MiniMap } from "@reactflow/minimap";
import "@reactflow/core/dist/style.css";
import {
  Plus, Save, Play, Loader2, CheckCircle2, XCircle,
  ChevronDown, ChevronRight, X as CloseIcon,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import type { Flow, FlowExecution, NodeResult } from "@/lib/tauri";
import { CustomNode } from "./CustomNode";
import { NodeConfigPanel } from "./NodeConfigPanel";

interface FlowProgressEvent {
  node_id: string;
  node_label: string;
  step: number;
  total: number;
  status: "running" | "done" | "error" | "completed";
}

/** Stored after execution — the primary output text + all step details */
interface ExecutionResult {
  /** Primary output: the last output-node result, or last non-error result */
  primaryText: string;
  /** Whether primaryText should be rendered as markdown */
  isMarkdown: boolean;
  /** All node results for the steps accordion */
  steps: Array<NodeResult & { label: string; nodeType: string }>;
  hasError: boolean;
}

/** Serialize a NodeResult output value to a readable string */
function outputToString(output: unknown): string {
  if (output === null || output === undefined) return "(empty)";
  if (typeof output === "string") return output;
  return JSON.stringify(output, null, 2);
}

const nodeTypes: NodeTypes = {
  customNode: CustomNode,
};

const NODE_PALETTE = [
  { type: "trigger", label: "Trigger", color: "bg-rose-500/20 border-rose-500/30 text-rose-300" },
  { type: "input", label: "Input", color: "bg-green-500/20 border-green-500/30 text-green-300" },
  { type: "system_prompt", label: "System Prompt", color: "bg-purple-500/20 border-purple-500/30 text-purple-300" },
  { type: "user_prompt", label: "User Prompt", color: "bg-blue-500/20 border-blue-500/30 text-blue-300" },
  { type: "template_prompt", label: "Template", color: "bg-indigo-500/20 border-indigo-500/30 text-indigo-300" },
  { type: "web_search", label: "Web Search", color: "bg-emerald-500/20 border-emerald-500/30 text-emerald-300" },
  { type: "code_exec", label: "Code Exec", color: "bg-orange-500/20 border-orange-500/30 text-orange-300" },
  { type: "http_api", label: "HTTP API", color: "bg-cyan-500/20 border-cyan-500/30 text-cyan-300" },
  { type: "db_query", label: "DB Query", color: "bg-amber-500/20 border-amber-500/30 text-amber-300" },
  { type: "conditional", label: "Condition", color: "bg-yellow-500/20 border-yellow-500/30 text-yellow-300" },
  { type: "loop", label: "Loop", color: "bg-pink-500/20 border-pink-500/30 text-pink-300" },
  { type: "merge", label: "Merge", color: "bg-teal-500/20 border-teal-500/30 text-teal-300" },
  { type: "output", label: "Output", color: "bg-red-500/20 border-red-500/30 text-red-300" },
];

function getDefaultNodeData(nodeType: string): Record<string, unknown> {
  const palette = NODE_PALETTE.find((n) => n.type === nodeType);
  const base = {
    label: palette?.label || nodeType,
    nodeType,
    color: palette?.color || "",
  };

  switch (nodeType) {
    case "trigger":
      return { ...base, trigger_type: "manual", schedule_cron: "", webhook_path: "", description: "" };
    case "input":
      return { ...base, variable: "input", default_value: "", description: "" };
    case "system_prompt":
      return { ...base, prompt: "", description: "" };
    case "user_prompt":
      return { ...base, prompt: "", temperature: 0.7, max_tokens: 2048, top_p: 0.9, description: "" };
    case "template_prompt":
      return { ...base, prompt: "", temperature: 0.7, max_tokens: 2048, top_p: 0.9, description: "" };
    case "web_search":
      return { ...base, query: "", max_results: 5, description: "" };
    case "code_exec":
      return { ...base, command: "", language: "bash", timeout_seconds: 30, description: "" };
    case "http_api":
      return { ...base, method: "GET", url: "", headers: "", body: "", content_type: "application/json", description: "" };
    case "db_query":
      return { ...base, query: "", connection_id: "", db_type: "postgresql", description: "" };
    case "conditional":
      return { ...base, condition: "", true_label: "True", false_label: "False", description: "" };
    case "loop":
      return { ...base, iterations: 1, loop_variable: "i", collection_variable: "", description: "" };
    case "merge":
      return { ...base, merge_strategy: "concat", separator: "\\n", description: "" };
    case "output":
      return { ...base, variable: "last_response", format: "text", description: "" };
    default:
      return base;
  }
}

let nodeIdCounter = 1;

export function FlowCanvas() {
  const [flows, setFlows] = useState<Flow[]>([]);
  const [activeFlow, setActiveFlow] = useState<Flow | null>(null);
  const [nodes, setNodes, onNodesChange] = useNodesState([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState([]);
  const [flowName, setFlowName] = useState("New Flow");
  const [isExecuting, setIsExecuting] = useState(false);
  const [execResult, setExecResult] = useState<ExecutionResult | null>(null);
  const [showSteps, setShowSteps] = useState(false);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  /** node_id → exec status, cleared after execution */
  const [nodeExecStatus, setNodeExecStatus] = useState<Record<string, "running" | "done" | "error">>({});
  const [execStep, setExecStep] = useState<{ step: number; total: number; label: string } | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const execStatusTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Clean up listener and any pending timers if the component unmounts mid-execution
  useEffect(() => {
    return () => {
      unlistenRef.current?.();
      if (execStatusTimerRef.current) clearTimeout(execStatusTimerRef.current);
    };
  }, []);

  const selectedNode = useMemo(
    () => nodes.find((n) => n.id === selectedNodeId) || null,
    [nodes, selectedNodeId]
  );

  // Inject _execStatus into each node's data for CustomNode to read
  const displayNodes = useMemo(
    () =>
      nodes.map((n) => ({
        ...n,
        data: {
          ...n.data,
          _execStatus: nodeExecStatus[n.id] ?? null,
        },
      })),
    [nodes, nodeExecStatus]
  );

  const onNodeClick = useCallback((_: React.MouseEvent, node: Node) => {
    setSelectedNodeId(node.id);
  }, []);

  const onPaneClick = useCallback(() => {
    setSelectedNodeId(null);
  }, []);

  const onNodeDataUpdate = useCallback(
    (nodeId: string, newData: Record<string, unknown>) => {
      setNodes((nds) =>
        nds.map((n) => (n.id === nodeId ? { ...n, data: newData } : n))
      );
    },
    [setNodes]
  );

  const deleteNode = useCallback(
    (nodeId: string) => {
      setNodes((nds) => nds.filter((n) => n.id !== nodeId));
      setEdges((eds) => eds.filter((e) => e.source !== nodeId && e.target !== nodeId));
      setSelectedNodeId((prev) => (prev === nodeId ? null : prev));
    },
    [setNodes, setEdges]
  );

  useEffect(() => {
    fetchFlows();
  }, []);

  const fetchFlows = async () => {
    try {
      const result = await invoke<Flow[]>("list_flows");
      setFlows(result);
    } catch (e) {
      console.error(e);
    }
  };

  const onConnect = useCallback(
    (params: Connection) => setEdges((eds) => addEdge(params, eds)),
    [setEdges]
  );

  const onDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
  };

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    const nodeType = e.dataTransfer.getData("nodeType");
    if (!nodeType) return;

    const bounds = (e.currentTarget as HTMLDivElement).getBoundingClientRect();
    const position = { x: e.clientX - bounds.left - 75, y: e.clientY - bounds.top - 30 };

    const newNode = {
      id: `node_${nodeIdCounter++}`,
      type: "customNode",
      position,
      data: { ...getDefaultNodeData(nodeType), onDelete: deleteNode },
    };

    setNodes((nds) => [...nds, newNode]);
  };

  const loadFlow = (flow: Flow) => {
    setActiveFlow(flow);
    setFlowName(flow.name);
    setSelectedNodeId(null);
    setNodes(flow.nodes.map((n) => ({
      id: n.id,
      type: "customNode",
      position: { x: n.position_x, y: n.position_y },
      data: { ...(n.data as Record<string, unknown>), onDelete: deleteNode },
    })));
    setEdges(flow.edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
    })));
  };

  const saveFlow = async () => {
    const flowData: Flow = {
      id: activeFlow?.id || "",
      name: flowName,
      description: null,
      nodes: nodes.map((n) => ({
        id: n.id,
        node_type: (n.data as Record<string, unknown>).nodeType as string,
        position_x: n.position.x,
        position_y: n.position.y,
        data: n.data as Record<string, unknown>,
      })),
      edges: edges.map((e) => ({
        id: e.id,
        source: e.source,
        target: e.target,
        source_handle: e.sourceHandle || null,
        target_handle: e.targetHandle || null,
      })),
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };

    try {
      const saved = await invoke<Flow>("save_flow", { flow: flowData });
      setActiveFlow(saved);
      await fetchFlows();
    } catch (e) {
      console.error(e);
    }
  };

  const executeFlow = async () => {
    if (!activeFlow) return;
    setIsExecuting(true);
    setExecResult(null);
    setShowSteps(false);
    setNodeExecStatus({});
    setExecStep(null);

    // Subscribe to per-node progress events
    const unlisten = await listen<FlowProgressEvent>("flow_progress", (event) => {
      const { node_id, node_label, step, total, status } = event.payload;
      if (status === "completed") {
        setExecStep(null);
        return;
      }
      setExecStep({ step, total, label: node_label });
      if (node_id) {
        setNodeExecStatus((prev) => ({ ...prev, [node_id]: status as "running" | "done" | "error" }));
      }
    });
    unlistenRef.current = unlisten;

    try {
      const result = await invoke<FlowExecution>("execute_flow", {
        flowId: activeFlow.id,
        input: null,
      });

      // ── Find the best result to display ──────────────────────────────────
      // Priority: last output-type node → last user_prompt/template_prompt →
      // last non-error node.
      const outputNodeIds = new Set(
        activeFlow.nodes
          .filter((n) => n.node_type === "output")
          .map((n) => n.id)
      );
      const llmNodeIds = new Set(
        activeFlow.nodes
          .filter((n) => n.node_type === "user_prompt" || n.node_type === "template_prompt")
          .map((n) => n.id)
      );

      const findLast = <T,>(arr: T[], pred: (x: T) => boolean) =>
        [...arr].reverse().find(pred);
      const best =
        findLast(result.node_results, (r) => !r.error && outputNodeIds.has(r.node_id)) ??
        findLast(result.node_results, (r) => !r.error && llmNodeIds.has(r.node_id)) ??
        findLast(result.node_results, (r) => !r.error);

      // Determine output format: check the output node's format field
      const outputNode = activeFlow.nodes.find(
        (n) => n.node_type === "output" && best?.node_id === n.id
      );
      const format = (outputNode?.data as Record<string, unknown>)?.format as string | undefined;
      const isMarkdown = !format || format === "markdown" || format === "text";

      // Annotate each step with its label and node type for the steps view
      const nodeById = new Map(activeFlow.nodes.map((n) => [n.id, n]));
      const steps = result.node_results.map((r) => {
        const flowNode = nodeById.get(r.node_id);
        return {
          ...r,
          label: (flowNode?.data as Record<string, unknown>)?.label as string ?? r.node_id,
          nodeType: flowNode?.node_type ?? "unknown",
        };
      });

      setExecResult({
        primaryText: best ? outputToString(best.output) : "Completed with no output.",
        isMarkdown,
        steps,
        hasError: result.node_results.some((r) => r.error),
      });
    } catch (e) {
      setExecResult({
        primaryText: `Execution error: ${e}`,
        isMarkdown: false,
        steps: [],
        hasError: true,
      });
    } finally {
      setIsExecuting(false);
      setExecStep(null);
      if (execStatusTimerRef.current) clearTimeout(execStatusTimerRef.current);
      execStatusTimerRef.current = setTimeout(() => setNodeExecStatus({}), 2500);
      unlisten();
      unlistenRef.current = null;
    }
  };

  const newFlow = () => {
    setActiveFlow(null);
    setFlowName("New Flow");
    setSelectedNodeId(null);
    setNodes([]);
    setEdges([]);
    setExecResult(null);
    setShowSteps(false);
  };

  return (
    <div className="flex h-full">
      {/* Flow list sidebar */}
      <div className="w-48 flex flex-col border-r border-border bg-card/50">
        <div className="flex items-center justify-between p-3 border-b border-border">
          <span className="text-sm font-semibold">Flows</span>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={newFlow}>
            <Plus className="w-4 h-4" />
          </Button>
        </div>
        <ScrollArea className="flex-1">
          <div className="p-2 space-y-1">
            {flows.map((flow) => (
              <div
                key={flow.id}
                className={cn(
                  "px-3 py-2 rounded-lg cursor-pointer text-xs transition-colors",
                  activeFlow?.id === flow.id
                    ? "bg-primary/10 text-foreground"
                    : "hover:bg-secondary text-muted-foreground"
                )}
                onClick={() => loadFlow(flow)}
              >
                {flow.name}
              </div>
            ))}
          </div>
        </ScrollArea>
      </div>

      {/* Canvas area */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Toolbar */}
        <div className="flex items-center gap-2 px-4 py-2 border-b border-border bg-card/50">
          <input
            className="flex-1 bg-transparent text-sm font-medium outline-none border-b border-transparent focus:border-primary/50 transition-colors px-1"
            value={flowName}
            onChange={(e) => setFlowName(e.target.value)}
          />
          <Button size="sm" variant="outline" onClick={saveFlow}>
            <Save className="w-3.5 h-3.5 mr-1" />
            Save
          </Button>
          <Button
            size="sm"
            disabled={!activeFlow || isExecuting}
            onClick={executeFlow}
          >
            {isExecuting ? (
              <Loader2 className="w-3.5 h-3.5 mr-1 animate-spin" />
            ) : (
              <Play className="w-3.5 h-3.5 mr-1" />
            )}
            Run
          </Button>
          {isExecuting && execStep && (
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground animate-pulse ml-1">
              <Loader2 className="w-3 h-3 animate-spin text-yellow-400" />
              <span className="text-yellow-400 font-medium">
                {execStep.step}/{execStep.total}
              </span>
              <span className="truncate max-w-[140px]">{execStep.label}</span>
            </div>
          )}
          {!isExecuting && Object.values(nodeExecStatus).length > 0 && (
            <div className="flex items-center gap-1 text-xs ml-1">
              {Object.values(nodeExecStatus).some((s) => s === "error") ? (
                <><XCircle className="w-3.5 h-3.5 text-destructive" /><span className="text-destructive">Failed</span></>
              ) : (
                <><CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" /><span className="text-emerald-400">Done</span></>
              )}
            </div>
          )}
        </div>

        <div className="flex flex-1 min-h-0">
          {/* Node palette */}
          <div className="w-44 border-r border-border bg-card/30 p-2 overflow-y-auto">
            <div className="text-[10px] text-muted-foreground font-semibold uppercase tracking-wider mb-2 px-1">
              Nodes
            </div>
            <div className="space-y-1">
              {NODE_PALETTE.map((item) => (
                <div
                  key={item.type}
                  draggable
                  onDragStart={(e) => e.dataTransfer.setData("nodeType", item.type)}
                  className={cn(
                    "px-3 py-2 rounded-md text-xs border cursor-grab active:cursor-grabbing transition-colors",
                    item.color
                  )}
                >
                  {item.label}
                </div>
              ))}
            </div>
          </div>

          {/* ReactFlow canvas */}
          <div className="flex-1 relative" onDragOver={onDragOver} onDrop={onDrop}>
            <ReactFlow
              nodes={displayNodes}
              edges={edges}
              onNodesChange={onNodesChange}
              onEdgesChange={onEdgesChange}
              onConnect={onConnect}
              onNodeClick={onNodeClick}
              onPaneClick={onPaneClick}
              nodeTypes={nodeTypes}
              fitView
              deleteKeyCode="Delete"
            >
              <Background variant={BackgroundVariant.Dots} gap={20} size={1} />
              <Controls />
              <MiniMap nodeStrokeWidth={3} />
            </ReactFlow>
          </div>

          {selectedNode && (
            <NodeConfigPanel
              node={selectedNode}
              onUpdate={onNodeDataUpdate}
              onClose={() => setSelectedNodeId(null)}
              onDelete={deleteNode}
            />
          )}
        </div>

        {/* Result panel */}
        {execResult && (
          <div className="border-t border-border bg-card/50 flex flex-col" style={{ maxHeight: "45%" }}>
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-border shrink-0">
              <div className="flex items-center gap-2">
                {execResult.hasError ? (
                  <XCircle className="w-3.5 h-3.5 text-destructive" />
                ) : (
                  <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
                )}
                <span className="text-xs font-semibold">
                  {execResult.hasError ? "Execution completed with errors" : "Execution Result"}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => setShowSteps((v) => !v)}
                  className="flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
                >
                  {showSteps ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                  {execResult.steps.length} steps
                </button>
                <button
                  onClick={() => setExecResult(null)}
                  className="p-0.5 rounded text-muted-foreground hover:text-foreground transition-colors"
                  title="Dismiss"
                >
                  <CloseIcon className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>

            <div className="flex-1 overflow-y-auto">
              {/* Steps accordion */}
              {showSteps && (
                <div className="border-b border-border">
                  {execResult.steps.map((step, i) => (
                    <div
                      key={step.node_id}
                      className={cn(
                        "flex items-start gap-2 px-4 py-1.5 text-[11px] border-b border-border/50 last:border-0",
                        step.error ? "bg-destructive/5" : i % 2 === 0 ? "bg-transparent" : "bg-secondary/20"
                      )}
                    >
                      <span
                        className={cn(
                          "shrink-0 mt-0.5",
                          step.error ? "text-destructive" : "text-emerald-400"
                        )}
                      >
                        {step.error ? "✗" : "✓"}
                      </span>
                      <span className="text-muted-foreground font-medium shrink-0 w-32 truncate">
                        {step.label}
                      </span>
                      <span className="text-muted-foreground/60 shrink-0">{step.duration_ms}ms</span>
                      <span className="truncate text-muted-foreground/80 flex-1">
                        {step.error ?? outputToString(step.output).slice(0, 120)}
                      </span>
                    </div>
                  ))}
                </div>
              )}

              {/* Primary output */}
              <div className="px-4 py-3">
                {execResult.isMarkdown ? (
                  <div className="prose prose-sm prose-invert max-w-none text-sm">
                    <ReactMarkdown>{execResult.primaryText}</ReactMarkdown>
                  </div>
                ) : (
                  <pre className="text-xs whitespace-pre-wrap font-mono text-foreground/90">
                    {execResult.primaryText}
                  </pre>
                )}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
