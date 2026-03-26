import { useEffect, useState } from "react";
import { Plus, Trash2, Play, Database, Loader2, CheckCircle } from "lucide-react";
import { invoke } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import type { DbConnection, QueryResult } from "@/lib/tauri";

export function DatabaseView() {
  const [connections, setConnections] = useState<DbConnection[]>([]);
  const [activeConn, setActiveConn] = useState<DbConnection | null>(null);
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<QueryResult | null>(null);
  const [isQuerying, setIsQuerying] = useState(false);
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [addForm, setAddForm] = useState({ name: "", db_type: "postgresql", connection_string: "" });
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);

  useEffect(() => {
    fetchConnections();
  }, []);

  const fetchConnections = async () => {
    try {
      const conns = await invoke<DbConnection[]>("list_db_connections");
      setConnections(conns);
    } catch (e) {
      console.error(e);
    }
  };

  const handleAddConnection = async () => {
    try {
      await invoke("add_db_connection", {
        name: addForm.name,
        dbType: addForm.db_type,
        connectionString: addForm.connection_string,
      });
      await fetchConnections();
      setShowAddDialog(false);
      setAddForm({ name: "", db_type: "postgresql", connection_string: "" });
    } catch (e) {
      setTestResult(`Error: ${e}`);
    }
  };

  const handleTestConnection = async () => {
    setIsTesting(true);
    setTestResult(null);
    try {
      const ok = await invoke<boolean>("test_db_connection", {
        connectionString: addForm.connection_string,
        dbType: addForm.db_type,
      });
      setTestResult(ok ? "Connection successful!" : "Connection failed.");
    } catch (e) {
      setTestResult(`Error: ${e}`);
    } finally {
      setIsTesting(false);
    }
  };

  const handleQuery = async () => {
    if (!activeConn || !query.trim()) return;
    setIsQuerying(true);
    setResult(null);
    try {
      const res = await invoke<QueryResult>("execute_db_query", {
        connectionId: activeConn.id,
        query: query.trim(),
      });
      setResult(res);
    } catch (e) {
      console.error(e);
    } finally {
      setIsQuerying(false);
    }
  };

  const dbTypeColor = (type: string) => {
    switch (type) {
      case "mongodb": return "bg-green-500/20 text-green-400";
      case "mysql": return "bg-blue-500/20 text-blue-400";
      default: return "bg-indigo-500/20 text-indigo-400";
    }
  };

  return (
    <div className="flex h-full">
      {/* Connections sidebar */}
      <div className="w-64 flex flex-col border-r border-border bg-card/50">
        <div className="flex items-center justify-between p-3 border-b border-border">
          <span className="text-sm font-semibold">Connections</span>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={() => setShowAddDialog(true)}>
            <Plus className="w-4 h-4" />
          </Button>
        </div>
        <ScrollArea className="flex-1">
          <div className="p-2 space-y-1">
            {connections.map((conn) => (
              <div
                key={conn.id}
                className={cn(
                  "group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-colors",
                  activeConn?.id === conn.id
                    ? "bg-primary/10 text-foreground"
                    : "hover:bg-secondary text-muted-foreground hover:text-foreground"
                )}
                onClick={() => setActiveConn(conn)}
              >
                <Database className="w-3.5 h-3.5 shrink-0" />
                <div className="flex-1 min-w-0">
                  <div className="text-xs font-medium truncate">{conn.name}</div>
                  <Badge className={cn("text-[9px] px-1 py-0 mt-0.5", dbTypeColor(conn.db_type))}>
                    {conn.db_type}
                  </Badge>
                </div>
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-5 w-5 opacity-0 group-hover:opacity-100 text-destructive"
                  onClick={async (e) => {
                    e.stopPropagation();
                    await invoke("delete_db_connection", { connectionId: conn.id });
                    await fetchConnections();
                  }}
                >
                  <Trash2 className="w-3 h-3" />
                </Button>
              </div>
            ))}
            {connections.length === 0 && (
              <div className="text-center text-muted-foreground text-xs py-8">No connections. Add one to start.</div>
            )}
          </div>
        </ScrollArea>
      </div>

      {/* Query panel */}
      <div className="flex-1 flex flex-col min-w-0">
        <div className="px-6 py-4 border-b border-border">
          <h1 className="text-xl font-semibold">Database Connector</h1>
          <p className="text-sm text-muted-foreground mt-1">Query MongoDB, PostgreSQL, and MySQL databases</p>
        </div>

        {activeConn ? (
          <div className="flex-1 flex flex-col p-6 gap-4 overflow-hidden">
            <div className="flex items-center gap-2">
              <Database className="w-4 h-4 text-primary" />
              <span className="font-medium text-sm">{activeConn.name}</span>
              <Badge className={cn("text-[10px]", dbTypeColor(activeConn.db_type))}>{activeConn.db_type}</Badge>
            </div>

            {activeConn.db_type === "mongodb" && (
              <p className="text-xs text-muted-foreground">
                MongoDB format: <code className="font-mono">database.collection {"{\"field\": \"value\"}"}</code>
              </p>
            )}

            <Textarea
              className="font-mono text-sm resize-none"
              rows={6}
              placeholder={activeConn.db_type === "mongodb"
                ? 'mydb.users {"status": "active"}'
                : "SELECT * FROM users LIMIT 10;"
              }
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <Button className="self-start" onClick={handleQuery} disabled={isQuerying || !query.trim()}>
              {isQuerying ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : <Play className="w-4 h-4 mr-2" />}
              Execute Query
            </Button>

            {result && (
              <div className="flex-1 overflow-hidden flex flex-col min-h-0">
                <div className="text-sm font-medium mb-2 flex items-center gap-2">
                  Results
                  <Badge variant="secondary">{result.row_count} rows</Badge>
                  <span className="text-xs text-muted-foreground">{result.duration_ms}ms</span>
                </div>
                <ScrollArea className="flex-1 border border-border rounded-lg">
                  <div className="p-3">
                    {result.columns.length > 0 ? (
                      <table className="w-full text-xs">
                        <thead>
                          <tr className="border-b border-border">
                            {result.columns.map((col) => (
                              <th key={col} className="text-left pb-2 px-2 text-muted-foreground font-medium">{col}</th>
                            ))}
                          </tr>
                        </thead>
                        <tbody>
                          {result.rows.map((row, i) => (
                            <tr key={i} className="border-b border-border/50 hover:bg-secondary/30">
                              {result.columns.map((col) => (
                                <td key={col} className="py-1.5 px-2 truncate max-w-[200px]">
                                  {String(row[col] ?? "")}
                                </td>
                              ))}
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    ) : (
                      <pre className="text-xs whitespace-pre-wrap">
                        {JSON.stringify(result.rows, null, 2)}
                      </pre>
                    )}
                  </div>
                </ScrollArea>
              </div>
            )}
          </div>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center text-center p-8">
            <Database className="w-12 h-12 text-muted-foreground mb-3" />
            <p className="text-muted-foreground text-sm">Select a connection to run queries.</p>
          </div>
        )}
      </div>

      {/* Add connection dialog */}
      {showAddDialog && (
        <div className="fixed inset-0 bg-black/70 flex items-center justify-center z-50">
          <div className="bg-card border border-border rounded-xl p-6 w-full max-w-md mx-4">
            <h3 className="text-lg font-semibold mb-4">Add Database Connection</h3>
            <div className="space-y-3">
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Name</label>
                <Input value={addForm.name} onChange={(e) => setAddForm((f) => ({ ...f, name: e.target.value }))} placeholder="My Database" />
              </div>
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Type</label>
                <select
                  className="flex h-9 w-full rounded-md border border-input bg-background text-foreground px-3 py-1 text-sm [&>option]:bg-background [&>option]:text-foreground"
                  value={addForm.db_type}
                  onChange={(e) => setAddForm((f) => ({ ...f, db_type: e.target.value }))}
                >
                  <option value="postgresql">PostgreSQL</option>
                  <option value="mysql">MySQL</option>
                  <option value="mongodb">MongoDB</option>
                </select>
              </div>
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Connection String</label>
                <Input
                  value={addForm.connection_string}
                  onChange={(e) => setAddForm((f) => ({ ...f, connection_string: e.target.value }))}
                  placeholder={addForm.db_type === "mongodb"
                    ? "mongodb://user:pass@host:27017/db"
                    : `${addForm.db_type}://user:pass@host:5432/db`}
                />
              </div>
              {testResult && (
                <div className={cn("flex items-center gap-2 text-xs", testResult.includes("successful") ? "text-emerald-400" : "text-destructive")}>
                  <CheckCircle className="w-3.5 h-3.5" />
                  {testResult}
                </div>
              )}
            </div>
            <div className="flex justify-end gap-2 mt-4">
              <Button variant="outline" onClick={() => { setShowAddDialog(false); setTestResult(null); }}>Cancel</Button>
              <Button variant="secondary" onClick={handleTestConnection} disabled={isTesting || !addForm.connection_string}>
                {isTesting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : "Test"}
              </Button>
              <Button onClick={handleAddConnection} disabled={!addForm.name || !addForm.connection_string}>
                Add
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
