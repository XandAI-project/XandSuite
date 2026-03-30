import { useEffect, useState } from "react";
import { RefreshCw, Wrench, Server, ChevronDown, ChevronRight, Play, Loader2, AlertCircle } from "lucide-react";
import { useSkillsStore } from "@/stores/skillsStore";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import { invoke } from "@/lib/tauri";
import { McpServerManager } from "./McpServerManager";

export function SkillsPanel() {
  const { servers, tools, isLoading, error, fetchServers, fetchTools, removeMcpServer, reloadBuiltins, clearError } =
    useSkillsStore();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [testResult, setTestResult] = useState<Record<string, string>>({});
  const [testing, setTesting] = useState<string | null>(null);

  useEffect(() => {
    fetchServers();
    fetchTools();
  }, []);

  const toggleExpand = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  };

  const handleTestTool = async (serverId: string, toolName: string, toolKey: string) => {
    setTesting(toolKey);
    try {
      const result = await invoke<{ result: string; is_error: boolean }>("call_tool_direct", {
        request: { server_id: serverId, tool_name: toolName, arguments: {} },
      });
      setTestResult((prev) => ({ ...prev, [toolKey]: result.is_error ? `Error: ${result.result}` : result.result }));
    } catch (e) {
      setTestResult((prev) => ({ ...prev, [toolKey]: `Error: ${e}` }));
    } finally {
      setTesting(null);
    }
  };

  // Group tools by server
  const toolsByServer: Record<string, typeof tools> = {};
  for (const t of tools) {
    if (!toolsByServer[t.server_id]) toolsByServer[t.server_id] = [];
    toolsByServer[t.server_id].push(t);
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border">
        <div className="flex items-center gap-2">
          <Wrench className="w-4 h-4 text-primary" />
          <span className="text-sm font-semibold">Skills & Tools</span>
          <span className="text-xs text-muted-foreground">
            ({tools.length} tool{tools.length !== 1 ? "s" : ""})
          </span>
        </div>
        <div className="flex gap-1">
          <Button
            size="icon"
            variant="ghost"
            className="h-7 w-7"
            onClick={reloadBuiltins}
            disabled={isLoading}
            title="Reload builtin servers"
          >
            <RefreshCw className={cn("w-3.5 h-3.5", isLoading && "animate-spin")} />
          </Button>
          <Button size="sm" variant="outline" className="h-7 text-xs" onClick={() => setShowAddDialog(true)}>
            + Add MCP Server
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-4 mt-3 flex items-start gap-2 rounded-lg border border-destructive/50 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          <AlertCircle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
          <span className="flex-1">{error}</span>
          <button onClick={clearError} className="shrink-0 opacity-70 hover:opacity-100">✕</button>
        </div>
      )}

      <ScrollArea className="flex-1">
        <div className="p-3 space-y-2">
          {servers.length === 0 && !isLoading && (
            <div className="text-center py-12 text-muted-foreground text-xs">
              <Wrench className="w-8 h-8 mx-auto mb-3 opacity-20" />
              <div className="font-medium mb-1">No servers connected</div>
              <div>Click "Reload builtin servers" or add an external MCP server.</div>
            </div>
          )}

          {servers.map((srv) => {
            const serverTools = toolsByServer[srv.config.id] || [];
            const isOpen = expanded.has(srv.config.id);

            return (
              <div key={srv.config.id} className="rounded-xl border border-border bg-card/50 overflow-hidden">
                {/* Server header */}
                <button
                  onClick={() => toggleExpand(srv.config.id)}
                  className="flex w-full items-center gap-3 px-3 py-2.5 hover:bg-secondary/50 transition-colors text-left"
                >
                  <div className={cn(
                    "w-2 h-2 rounded-full shrink-0",
                    srv.connected ? "bg-emerald-400" : "bg-red-400"
                  )} />
                  <Server className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
                  <div className="flex-1 min-w-0">
                    <div className="text-xs font-semibold truncate">{srv.config.name}</div>
                    <div className="text-[10px] text-muted-foreground truncate">{srv.config.description}</div>
                  </div>
                  <span className="text-[10px] text-muted-foreground shrink-0">{serverTools.length} tool{serverTools.length !== 1 ? "s" : ""}</span>
                  {srv.config.builtin ? null : (
                    <button
                      onClick={(e) => { e.stopPropagation(); removeMcpServer(srv.config.id); }}
                      className="text-[10px] text-destructive opacity-70 hover:opacity-100 px-1 shrink-0"
                    >
                      Remove
                    </button>
                  )}
                  {isOpen ? <ChevronDown className="w-3 h-3 shrink-0" /> : <ChevronRight className="w-3 h-3 shrink-0" />}
                </button>

                {/* Tools list */}
                {isOpen && (
                  <div className="border-t border-border divide-y divide-border/50">
                    {serverTools.length === 0 && (
                      <div className="px-4 py-3 text-xs text-muted-foreground">No tools available.</div>
                    )}
                    {serverTools.map((tagged) => {
                      const key = `${tagged.server_id}::${tagged.tool.name}`;
                      const result = testResult[key];
                      return (
                        <div key={tagged.tool.name} className="px-4 py-2.5 space-y-1">
                          <div className="flex items-center gap-2">
                            <Wrench className="w-3 h-3 text-primary/70 shrink-0" />
                            <span className="text-xs font-medium">{tagged.tool.name}</span>
                            <Button
                              size="icon"
                              variant="ghost"
                              className="h-5 w-5 ml-auto"
                              onClick={() => handleTestTool(tagged.server_id, tagged.tool.name, key)}
                              disabled={testing === key}
                              title="Test tool with empty args"
                            >
                              {testing === key
                                ? <Loader2 className="w-3 h-3 animate-spin" />
                                : <Play className="w-3 h-3" />}
                            </Button>
                          </div>
                          {tagged.tool.description && (
                            <p className="text-[10px] text-muted-foreground leading-relaxed pl-5">
                              {tagged.tool.description}
                            </p>
                          )}
                          {result && (
                            <pre className="text-[10px] font-mono bg-black/20 rounded p-2 whitespace-pre-wrap break-all max-h-32 overflow-y-auto text-foreground/80 ml-5">
                              {(() => { try { return JSON.stringify(JSON.parse(result), null, 2); } catch { return result; } })()}
                            </pre>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </ScrollArea>

      {showAddDialog && <McpServerManager onClose={() => setShowAddDialog(false)} />}
    </div>
  );
}
