import { useState } from "react";
import { X, Globe, Terminal, Loader2, AlertCircle } from "lucide-react";
import { useSkillsStore, type AddServerRequest } from "@/stores/skillsStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

interface Props {
  onClose: () => void;
}

export function McpServerManager({ onClose }: Props) {
  const { addMcpServer, isLoading, error, clearError } = useSkillsStore();
  const [transport, setTransport] = useState<"stdio" | "http">("http");
  const [form, setForm] = useState({
    id: "",
    name: "",
    description: "",
    // HTTP
    url: "",
    auth: "",
    // Stdio
    command: "python",
    args: "",
  });

  const update = (k: keyof typeof form, v: string) => setForm((f) => ({ ...f, [k]: v }));

  const handleAdd = async () => {
    clearError();
    const req: AddServerRequest = {
      id: form.id || form.name.toLowerCase().replace(/\s+/g, "-"),
      name: form.name.trim(),
      description: form.description.trim(),
      transport,
      ...(transport === "http"
        ? {
            url: form.url.trim(),
            auth: form.auth.trim() || undefined,
          }
        : {
            command: form.command.trim(),
            args: form.args
              .split(/\s+/)
              .map((a) => a.trim())
              .filter(Boolean),
          }),
    };
    await addMcpServer(req);
    if (!error) onClose();
  };

  const valid =
    form.name.trim() &&
    (transport === "http" ? form.url.trim() : form.command.trim());

  return (
    <div className="fixed inset-0 bg-black/70 flex items-center justify-center z-50">
      <div className="bg-card border border-border rounded-xl p-6 w-full max-w-lg mx-4 shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between mb-5">
          <h3 className="text-base font-semibold">Add MCP Server</h3>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="w-4 h-4" />
          </Button>
        </div>

        {error && (
          <div className="mb-4 flex items-start gap-2 rounded-lg border border-destructive/50 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            <AlertCircle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
            <span>{error}</span>
          </div>
        )}

        {/* Transport selector */}
        <div className="flex gap-2 mb-4">
          {(["http", "stdio"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTransport(t)}
              className={cn(
                "flex items-center gap-2 flex-1 justify-center rounded-lg border px-4 py-2 text-sm transition-colors",
                transport === t
                  ? "border-primary bg-primary/10 text-foreground"
                  : "border-border text-muted-foreground hover:border-primary/50"
              )}
            >
              {t === "http" ? <Globe className="w-4 h-4" /> : <Terminal className="w-4 h-4" />}
              {t === "http" ? "HTTP (Remote)" : "Stdio (Local)"}
            </button>
          ))}
        </div>

        <div className="space-y-3">
          {/* Common fields */}
          <div>
            <label className="text-xs text-muted-foreground mb-1 block">Server Name *</label>
            <Input
              placeholder="My MCP Server"
              value={form.name}
              onChange={(e) => update("name", e.target.value)}
              className="h-8 text-xs"
            />
          </div>
          <div>
            <label className="text-xs text-muted-foreground mb-1 block">Description</label>
            <Input
              placeholder="What this server provides…"
              value={form.description}
              onChange={(e) => update("description", e.target.value)}
              className="h-8 text-xs"
            />
          </div>
          <div>
            <label className="text-xs text-muted-foreground mb-1 block">Server ID (auto-generated if blank)</label>
            <Input
              placeholder="my-mcp-server"
              value={form.id}
              onChange={(e) => update("id", e.target.value)}
              className="h-8 text-xs"
            />
          </div>

          {/* Transport-specific fields */}
          {transport === "http" ? (
            <>
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Server URL *</label>
                <Input
                  placeholder="https://mcp.example.com/mcp"
                  value={form.url}
                  onChange={(e) => update("url", e.target.value)}
                  className="h-8 text-xs"
                />
              </div>
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Bearer Token (optional)</label>
                <Input
                  type="password"
                  placeholder="sk-..."
                  value={form.auth}
                  onChange={(e) => update("auth", e.target.value)}
                  className="h-8 text-xs"
                />
              </div>
            </>
          ) : (
            <>
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Command *</label>
                <Input
                  placeholder="python"
                  value={form.command}
                  onChange={(e) => update("command", e.target.value)}
                  className="h-8 text-xs font-mono"
                />
              </div>
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Arguments (space-separated)</label>
                <Input
                  placeholder="/path/to/server.py"
                  value={form.args}
                  onChange={(e) => update("args", e.target.value)}
                  className="h-8 text-xs font-mono"
                />
              </div>
            </>
          )}
        </div>

        <div className="flex justify-end gap-2 mt-5">
          <Button variant="outline" onClick={onClose}>Cancel</Button>
          <Button onClick={handleAdd} disabled={!valid || isLoading}>
            {isLoading && <Loader2 className="w-3.5 h-3.5 mr-1.5 animate-spin" />}
            Connect
          </Button>
        </div>
      </div>
    </div>
  );
}
