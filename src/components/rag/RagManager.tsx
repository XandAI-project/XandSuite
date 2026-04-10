import { useEffect, useState } from "react";
import {
  Plus,
  Trash2,
  Upload,
  FileText,
  FolderOpen,
  Loader2,
  Search,
  Brain,
  Database,
  GitBranch,
  BarChart2,
  CheckCircle2,
  RefreshCw,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { formatDistanceToNow } from "date-fns";
import { useRagStore } from "@/stores/ragStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { invoke } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import type { RagCollection } from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settingsStore";

type Tab = "internal" | "external";

export function RagManager() {
  const [activeTab, setActiveTab] = useState<Tab>("external");

  return (
    <div className="flex h-full flex-col">
      {/* Tab switcher */}
      <div className="flex border-b border-border shrink-0">
        <button
          onClick={() => setActiveTab("internal")}
          className={cn(
            "flex items-center gap-1.5 px-5 py-3 text-sm font-medium transition-colors",
            activeTab === "internal"
              ? "border-b-2 border-primary text-foreground"
              : "text-muted-foreground hover:text-foreground"
          )}
        >
          <Brain className="w-3.5 h-3.5" />
          Internal
        </button>
        <button
          onClick={() => setActiveTab("external")}
          className={cn(
            "flex items-center gap-1.5 px-5 py-3 text-sm font-medium transition-colors",
            activeTab === "external"
              ? "border-b-2 border-primary text-foreground"
              : "text-muted-foreground hover:text-foreground"
          )}
        >
          <Database className="w-3.5 h-3.5" />
          Knowledge Bases
        </button>
      </div>

      <div className="flex-1 min-h-0">
        {activeTab === "internal" ? <InternalMemoryTab /> : <ExternalCollectionsTab />}
      </div>
    </div>
  );
}

// ── Internal Memory Tab ────────────────────────────────────────────────────────

function InternalMemoryTab() {
  const { entries, isLoading, fetchEntries, deleteEntry, clearAll } = useMemoryStore();
  const [confirmClear, setConfirmClear] = useState(false);

  useEffect(() => {
    fetchEntries();
  }, []);

  const handleClear = async () => {
    if (!confirmClear) {
      setConfirmClear(true);
      return;
    }
    await clearAll();
    setConfirmClear(false);
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-6 py-4 border-b border-border shrink-0">
        <h1 className="text-xl font-semibold">Internal Memory</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Key facts automatically extracted from your conversations and recalled in future chats.
        </p>
      </div>

      {isLoading ? (
        <div className="flex-1 flex items-center justify-center">
          <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
        </div>
      ) : entries.length === 0 ? (
        <div className="flex-1 flex flex-col items-center justify-center text-center p-8">
          <Brain className="w-12 h-12 text-muted-foreground mb-3" />
          <p className="text-muted-foreground text-sm">
            No memories yet. Start chatting and key facts will be remembered automatically.
          </p>
        </div>
      ) : (
        <>
          <ScrollArea className="flex-1 p-4">
            <div className="space-y-2">
              {entries.map((entry) => (
                <div
                  key={entry.id}
                  className="group flex items-start gap-3 p-3 rounded-lg border border-border bg-card/50"
                >
                  <Brain className="w-3.5 h-3.5 mt-0.5 shrink-0 text-primary/70" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm">{entry.content}</p>
                    <p className="text-[10px] text-muted-foreground mt-1">
                      {formatRelative(entry.created_at)}
                    </p>
                  </div>
                  <button
                    onClick={() => deleteEntry(entry.id)}
                    className="opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:text-destructive text-muted-foreground"
                    title="Delete memory"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
              ))}
            </div>
          </ScrollArea>
          <div className="p-4 border-t border-border shrink-0 flex justify-end">
            <Button
              variant={confirmClear ? "destructive" : "outline"}
              size="sm"
              onClick={handleClear}
              onBlur={() => setConfirmClear(false)}
            >
              <Trash2 className="w-3.5 h-3.5 mr-1.5" />
              {confirmClear ? "Confirm clear all?" : "Clear all memories"}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

function formatRelative(dateStr: string): string {
  try {
    return formatDistanceToNow(new Date(dateStr), { addSuffix: true });
  } catch {
    return dateStr;
  }
}

// ── External Collections Tab ───────────────────────────────────────────────────

function ExternalCollectionsTab() {
  const { collections, fetchCollections, createCollection, deleteCollection, ingestDocument } =
    useRagStore();
  const { settings, fetchSettings } = useSettingsStore();
  const graphRagEnabled = settings?.graph_rag_enabled ?? false;
  const [selectedCollection, setSelectedCollection] = useState<RagCollection | null>(null);
  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<
    { content: string; score: number; source: string; entities?: string[] }[]
  >([]);
  const [isSearching, setIsSearching] = useState(false);
  const [isIngesting, setIsIngesting] = useState(false);
  const [isSwitchingMode, setIsSwitchingMode] = useState(false);
  const [isReindexing, setIsReindexing] = useState(false);
  const [reindexError, setReindexError] = useState<string | null>(null);

  useEffect(() => {
    fetchCollections();
    fetchSettings();
  }, []);

  const handleCreate = async () => {
    if (!newName.trim()) return;
    await createCollection(newName.trim(), newDesc.trim() || undefined);
    setNewName("");
    setNewDesc("");
    setShowCreate(false);
  };

  const handleUpload = async () => {
    if (!selectedCollection) return;
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: "Documents", extensions: ["pdf", "csv", "json", "jsonl", "txt", "md"] },
        ],
      });
      if (selected && typeof selected === "string") {
        setIsIngesting(true);
        await ingestDocument(selectedCollection.id, selected);
        setIsIngesting(false);
      }
    } catch (e) {
      console.error(e);
      setIsIngesting(false);
    }
  };

  const handleSearch = async () => {
    if (!searchQuery.trim()) return;
    setIsSearching(true);
    try {
      const results = await invoke<{ content: string; score: number; source: string; entities?: string[] }[]>(
        "search_rag",
        {
          query: searchQuery,
          collectionId: selectedCollection?.id || null,
          topK: 5,
        }
      );
      setSearchResults(results);
    } catch (e) {
      console.error(e);
    } finally {
      setIsSearching(false);
    }
  };

  const handleSetRetrievalMode = async (mode: "hybrid" | "graph") => {
    if (!selectedCollection) return;
    setIsSwitchingMode(true);
    setReindexError(null);
    try {
      await invoke("set_collection_retrieval_mode", {
        collectionId: selectedCollection.id,
        mode,
      });
      await fetchCollections();
      const updated = collections.find((c) => c.id === selectedCollection.id);
      if (updated) setSelectedCollection(updated);
    } catch (e) {
      console.error(e);
    } finally {
      setIsSwitchingMode(false);
    }
  };

  const handleReindex = async () => {
    if (!selectedCollection) return;
    setIsReindexing(true);
    setReindexError(null);
    try {
      await invoke("reindex_collection", { collectionId: selectedCollection.id });
      await fetchCollections();
      const updated = collections.find((c) => c.id === selectedCollection.id);
      if (updated) setSelectedCollection(updated);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      setReindexError(msg);
    } finally {
      setIsReindexing(false);
    }
  };

  // Filter out the internal memory collection from the external list.
  const externalCollections = collections.filter((c) => c.id !== "xand_internal_memory");

  return (
    <div className="flex h-full">
      {/* Collections sidebar */}
      <div className="w-64 flex flex-col border-r border-border bg-card/50">
        <div className="flex items-center justify-between p-3 border-b border-border">
          <span className="text-sm font-semibold">Collections</span>
          <Button
            size="icon"
            variant="ghost"
            className="h-7 w-7"
            onClick={() => setShowCreate(true)}
          >
            <Plus className="w-4 h-4" />
          </Button>
        </div>
        <ScrollArea className="flex-1">
          <div className="p-2 space-y-1">
            {externalCollections.map((col) => (
              <div
                key={col.id}
                className={cn(
                  "group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-colors",
                  selectedCollection?.id === col.id
                    ? "bg-primary/10 text-foreground"
                    : "hover:bg-secondary text-muted-foreground hover:text-foreground"
                )}
                onClick={() => setSelectedCollection(col)}
              >
                <FolderOpen className="w-3.5 h-3.5 shrink-0" />
                <div className="flex-1 min-w-0">
                  <div className="text-xs font-medium truncate">{col.name}</div>
                  <div className="flex items-center gap-1 mt-0.5">
                    <span className="text-[10px] text-muted-foreground">{col.document_count} chunks</span>
                    {col.retrieval_mode === "graph" && (
                      <span className="text-[9px] px-1 py-0.5 rounded bg-purple-500/20 text-purple-300 font-medium">
                        Graph
                      </span>
                    )}
                  </div>
                </div>
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-5 w-5 opacity-0 group-hover:opacity-100 text-destructive"
                  onClick={(e) => {
                    e.stopPropagation();
                    deleteCollection(col.id);
                  }}
                >
                  <Trash2 className="w-3 h-3" />
                </Button>
              </div>
            ))}
          </div>
        </ScrollArea>
      </div>

      {/* Main panel */}
      <div className="flex-1 flex flex-col min-w-0">
        <div className="px-6 py-4 border-b border-border">
          <h1 className="text-xl font-semibold">Knowledge Bases</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Upload documents to create searchable knowledge bases for your AI models.
          </p>
        </div>

        {selectedCollection ? (
          <div className="flex-1 flex flex-col p-6 gap-4">
            <div className="flex items-center justify-between">
              <div>
                <h2 className="font-semibold">{selectedCollection.name}</h2>
                {selectedCollection.description && (
                  <p className="text-sm text-muted-foreground">{selectedCollection.description}</p>
                )}
              </div>
              <Button onClick={handleUpload} disabled={isIngesting}>
                {isIngesting ? (
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Upload className="w-4 h-4 mr-2" />
                )}
                Upload Document
              </Button>
            </div>

            {/* Retrieval mode selector */}
            <div className="flex items-center gap-3 p-3 rounded-lg border border-border bg-card/50">
              <div className="flex-1">
                <div className="text-xs font-medium mb-0.5">Retrieval Mode</div>
                <div className="text-[11px] text-muted-foreground">
                  Choose how this knowledge base is searched during chat.
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => handleSetRetrievalMode("hybrid")}
                  disabled={isSwitchingMode}
                  className={cn(
                    "flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors",
                    selectedCollection.retrieval_mode !== "graph"
                      ? "glass-primary text-white"
                      : "bg-secondary text-muted-foreground hover:text-foreground"
                  )}
                >
                  <BarChart2 className="w-3 h-3" />
                  Hybrid
                </button>
                <button
                  onClick={() => graphRagEnabled && handleSetRetrievalMode("graph")}
                  disabled={isSwitchingMode || !graphRagEnabled}
                  title={!graphRagEnabled ? "Enable GraphRAG in Settings → Knowledge Base" : undefined}
                  className={cn(
                    "flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors",
                    selectedCollection.retrieval_mode === "graph"
                      ? "bg-purple-600 text-white"
                      : graphRagEnabled
                        ? "bg-secondary text-muted-foreground hover:text-foreground"
                        : "bg-secondary/50 text-muted-foreground/50 cursor-not-allowed"
                  )}
                >
                  <GitBranch className="w-3 h-3" />
                  Graph
                </button>
                {isSwitchingMode && <Loader2 className="w-3.5 h-3.5 animate-spin text-muted-foreground" />}
              </div>
              {selectedCollection.retrieval_mode === "graph" && (
                <div className="ml-1">
                  {selectedCollection.graph_indexed ? (
                    <div className="flex items-center gap-1 text-[10px] text-green-400">
                      <CheckCircle2 className="w-3 h-3" />
                      Indexed
                    </div>
                  ) : isReindexing ? (
                    <div className="flex items-center gap-1 text-[10px] text-yellow-400">
                      <RefreshCw className="w-3 h-3 animate-spin" />
                      Indexing…
                    </div>
                  ) : (
                    <button
                      onClick={handleReindex}
                      className="flex items-center gap-1 text-[10px] text-yellow-400 hover:text-yellow-300 transition-colors"
                      title="Send documents to GraphRAG server for graph indexing"
                    >
                      <RefreshCw className="w-3 h-3" />
                      Not indexed — click to index
                    </button>
                  )}
                </div>
              )}
            </div>

            {reindexError && (
              <div className="text-[11px] text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
                {reindexError}
              </div>
            )}

            <p className="text-sm text-muted-foreground">
              Supported formats: PDF, CSV, JSON, JSONL, TXT, Markdown
            </p>

            {/* Search test */}
            <div className="mt-2">
              <div className="text-sm font-medium mb-2">Test Search</div>
              <div className="flex gap-2">
                <Input
                  placeholder="Search your knowledge base..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleSearch()}
                  className="flex-1"
                />
                <Button
                  onClick={handleSearch}
                  disabled={isSearching || !searchQuery.trim()}
                >
                  {isSearching ? (
                    <Loader2 className="w-4 h-4 animate-spin" />
                  ) : (
                    <Search className="w-4 h-4" />
                  )}
                </Button>
              </div>

              {searchResults.length > 0 && (
                <ScrollArea className="mt-3 max-h-96">
                  <div className="space-y-2">
                    {searchResults.map((r, i) => (
                      <div key={i} className="p-3 rounded-lg border border-border bg-card/50">
                        <div className="flex items-center justify-between mb-1">
                          <div className="flex items-center gap-1.5">
                            <FileText className="w-3 h-3 text-muted-foreground" />
                            <span className="text-[10px] text-muted-foreground">{r.source}</span>
                          </div>
                          <Badge variant="secondary" className="text-[10px]">
                            {(r.score * 100).toFixed(0)}% match
                          </Badge>
                        </div>
                        <p className="text-xs">
                          {r.content.slice(0, 300)}
                          {r.content.length > 300 ? "..." : ""}
                        </p>
                        {r.entities && r.entities.length > 0 && (
                          <div className="flex flex-wrap gap-1 mt-1.5">
                            {r.entities.slice(0, 5).map((entity, ei) => (
                              <span key={ei} className="text-[9px] px-1.5 py-0.5 rounded bg-purple-500/15 text-purple-300 border border-purple-500/20">
                                {entity}
                              </span>
                            ))}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                </ScrollArea>
              )}
            </div>
          </div>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center text-center p-8">
            <FileText className="w-12 h-12 text-muted-foreground mb-3" />
            <p className="text-muted-foreground text-sm">
              Select a collection or create one to start ingesting documents.
            </p>
          </div>
        )}
      </div>

      {/* Create collection dialog */}
      {showCreate && (
        <div className="fixed inset-0 bg-black/70 flex items-center justify-center z-50">
          <div className="bg-card border border-border rounded-xl p-6 w-full max-w-md mx-4">
            <h3 className="text-lg font-semibold mb-4">New Knowledge Base</h3>
            <div className="space-y-3">
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Name</label>
                <Input
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  placeholder="e.g. Product Documentation"
                  autoFocus
                />
              </div>
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">
                  Description (optional)
                </label>
                <Input
                  value={newDesc}
                  onChange={(e) => setNewDesc(e.target.value)}
                  placeholder="What this collection contains..."
                />
              </div>
            </div>
            <div className="flex justify-end gap-2 mt-4">
              <Button variant="outline" onClick={() => setShowCreate(false)}>
                Cancel
              </Button>
              <Button onClick={handleCreate} disabled={!newName.trim()}>
                Create
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
