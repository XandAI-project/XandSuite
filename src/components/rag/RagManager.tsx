import { useEffect, useState } from "react";
import { Plus, Trash2, Upload, FileText, FolderOpen, Loader2, Search } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { useRagStore } from "@/stores/ragStore";
import { invoke } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import type { RagCollection } from "@/lib/tauri";

export function RagManager() {
  const { collections, fetchCollections, createCollection, deleteCollection, ingestDocument } = useRagStore();
  const [selectedCollection, setSelectedCollection] = useState<RagCollection | null>(null);
  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<{ content: string; score: number; source: string }[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [isIngesting, setIsIngesting] = useState(false);

  useEffect(() => {
    fetchCollections();
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
        filters: [{ name: "Documents", extensions: ["pdf", "csv", "json", "jsonl", "txt", "md"] }],
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
      const results = await invoke<{ content: string; score: number; source: string }[]>("search_rag", {
        query: searchQuery,
        collectionId: selectedCollection?.id || null,
        topK: 5,
      });
      setSearchResults(results);
    } catch (e) {
      console.error(e);
    } finally {
      setIsSearching(false);
    }
  };

  return (
    <div className="flex h-full">
      {/* Collections sidebar */}
      <div className="w-64 flex flex-col border-r border-border bg-card/50">
        <div className="flex items-center justify-between p-3 border-b border-border">
          <span className="text-sm font-semibold">Collections</span>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={() => setShowCreate(true)}>
            <Plus className="w-4 h-4" />
          </Button>
        </div>
        <ScrollArea className="flex-1">
          <div className="p-2 space-y-1">
            {collections.map((col) => (
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
                  <div className="text-[10px] text-muted-foreground">{col.document_count} chunks</div>
                </div>
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-5 w-5 opacity-0 group-hover:opacity-100 text-destructive"
                  onClick={(e) => { e.stopPropagation(); deleteCollection(col.id); }}
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
          <h1 className="text-xl font-semibold">RAG Manager</h1>
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
                <Button onClick={handleSearch} disabled={isSearching || !searchQuery.trim()}>
                  {isSearching ? <Loader2 className="w-4 h-4 animate-spin" /> : <Search className="w-4 h-4" />}
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
                        <p className="text-xs">{r.content.slice(0, 300)}{r.content.length > 300 ? "..." : ""}</p>
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
            <h3 className="text-lg font-semibold mb-4">New Collection</h3>
            <div className="space-y-3">
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Name</label>
                <Input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="My Knowledge Base" autoFocus />
              </div>
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Description (optional)</label>
                <Input value={newDesc} onChange={(e) => setNewDesc(e.target.value)} placeholder="What this collection contains..." />
              </div>
            </div>
            <div className="flex justify-end gap-2 mt-4">
              <Button variant="outline" onClick={() => setShowCreate(false)}>Cancel</Button>
              <Button onClick={handleCreate} disabled={!newName.trim()}>Create</Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
