import { useEffect, useRef, useState, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import {
  Search, Code, FileText, Globe, AlignLeft, Trash2,
  Download, Copy, Check, X, Layers, RefreshCw,
} from "lucide-react";
import { useArtifactStore } from "@/stores/artifactStore";
import { useChatStore } from "@/stores/chatStore";
import { cn } from "@/lib/utils";
import type { Artifact, ArtifactType } from "@/lib/tauri";

// ── helpers ──────────────────────────────────────────────────────────────────

const TYPE_META: Record<ArtifactType, { label: string; icon: React.ReactNode; color: string }> = {
  code:     { label: "Code",     icon: <Code      className="w-3.5 h-3.5" />, color: "text-blue-400 bg-blue-400/10 border-blue-400/20" },
  markdown: { label: "Document", icon: <FileText  className="w-3.5 h-3.5" />, color: "text-emerald-400 bg-emerald-400/10 border-emerald-400/20" },
  html:     { label: "HTML",     icon: <Globe     className="w-3.5 h-3.5" />, color: "text-orange-400 bg-orange-400/10 border-orange-400/20" },
  text:     { label: "Text",     icon: <AlignLeft className="w-3.5 h-3.5" />, color: "text-muted-foreground bg-secondary border-border" },
};

function formatDate(iso: string) {
  const d = new Date(iso);
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

// ── Preview pane ──────────────────────────────────────────────────────────────

function ArtifactPreview({ artifact }: { artifact: Artifact }) {
  const type = artifact.artifact_type as ArtifactType;

  if (type === "code") {
    return (
      <SyntaxHighlighter
        style={vscDarkPlus as Record<string, React.CSSProperties>}
        language={artifact.language ?? "text"}
        PreTag="div"
        className="!rounded-none !m-0 !text-xs !h-full !overflow-auto"
        showLineNumbers
        wrapLines
      >
        {artifact.content}
      </SyntaxHighlighter>
    );
  }

  if (type === "html") {
    return (
      <iframe
        srcDoc={artifact.content}
        sandbox="allow-scripts allow-same-origin"
        className="w-full h-full border-none bg-white"
        title={artifact.title}
      />
    );
  }

  if (type === "markdown") {
    return (
      <div className="p-5 overflow-auto h-full prose prose-invert prose-sm max-w-none">
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={{
            code({ className, children }) {
              const match = /language-(\w+)/.exec(className || "");
              const isBlock = String(children).includes("\n");
              return isBlock ? (
                <SyntaxHighlighter
                  style={vscDarkPlus as Record<string, React.CSSProperties>}
                  language={match ? match[1] : "text"}
                  PreTag="div"
                  className="!rounded-lg !my-2 !text-xs"
                >
                  {String(children).replace(/\n$/, "")}
                </SyntaxHighlighter>
              ) : (
                <code className="px-1 py-0.5 rounded bg-muted text-xs font-mono">{children}</code>
              );
            },
          }}
        >
          {artifact.content}
        </ReactMarkdown>
      </div>
    );
  }

  return (
    <pre className="p-5 text-xs font-mono whitespace-pre-wrap break-words overflow-auto h-full text-foreground">
      {artifact.content}
    </pre>
  );
}

// ── Main view ─────────────────────────────────────────────────────────────────

type Filter = "all" | ArtifactType;

export function ArtifactsView() {
  const { fetchAllArtifacts, deleteArtifact } = useArtifactStore();
  const isStreaming = useChatStore((s) => s.isStreaming);

  const [allArtifacts, setAllArtifacts] = useState<Artifact[]>([]);
  const [selected, setSelected] = useState<Artifact | null>(null);
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [copied, setCopied] = useState(false);
  const [loading, setLoading] = useState(true);

  const refresh = () => {
    setLoading(true);
    fetchAllArtifacts().then((data) => {
      setAllArtifacts(data);
      if (data.length > 0 && !selected) setSelected(data[0]);
      setLoading(false);
    });
  };

  // Initial load
  useEffect(() => {
    refresh();
  }, []);

  // Re-fetch whenever the AI finishes a streaming turn (artifacts may have been saved)
  const prevStreamingRef = useRef(false);
  useEffect(() => {
    const wasStreaming = prevStreamingRef.current;
    prevStreamingRef.current = isStreaming;
    if (wasStreaming && !isStreaming) {
      fetchAllArtifacts().then((data) => setAllArtifacts(data));
    }
  }, [isStreaming]);

  const filtered = useMemo(() => {
    let list = allArtifacts;
    if (filter !== "all") list = list.filter((a) => a.artifact_type === filter);
    if (search.trim()) {
      const q = search.toLowerCase();
      list = list.filter(
        (a) =>
          a.title.toLowerCase().includes(q) ||
          a.content.toLowerCase().includes(q) ||
          (a.language ?? "").toLowerCase().includes(q)
      );
    }
    return list;
  }, [allArtifacts, filter, search]);

  const handleDelete = async (a: Artifact) => {
    await deleteArtifact(a.id);
    setAllArtifacts((prev) => prev.filter((x) => x.id !== a.id));
    if (selected?.id === a.id) {
      const next = allArtifacts.find((x) => x.id !== a.id) ?? null;
      setSelected(next);
    }
  };

  const handleDownload = (a: Artifact) => {
    const ext =
      a.artifact_type === "code" ? a.language ?? "txt"
      : a.artifact_type === "html" ? "html"
      : a.artifact_type === "markdown" ? "md"
      : "txt";
    const blob = new Blob([a.content], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const el = document.createElement("a");
    el.href = url;
    el.download = `${a.title.replace(/\s+/g, "_")}.${ext}`;
    el.click();
    URL.revokeObjectURL(url);
  };

  const handleCopy = (a: Artifact) => {
    navigator.clipboard.writeText(a.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const filterButtons: { key: Filter; label: string }[] = [
    { key: "all",      label: "All" },
    { key: "code",     label: "Code" },
    { key: "markdown", label: "Document" },
    { key: "html",     label: "HTML" },
    { key: "text",     label: "Text" },
  ];

  const typeMeta = selected
    ? (TYPE_META[selected.artifact_type as ArtifactType] ?? TYPE_META.text)
    : null;

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Left: artifact list ────────────────────────────────────────── */}
      <div className="w-72 shrink-0 flex flex-col border-r border-border bg-card/50 h-full">
        {/* Header */}
        <div className="flex items-center gap-2 px-4 h-14 border-b border-border shrink-0">
          <Layers className="w-4 h-4 text-primary" />
          <span className="text-sm font-semibold">All Artifacts</span>
          {allArtifacts.length > 0 && (
            <span className="text-[10px] bg-secondary text-muted-foreground px-1.5 py-0.5 rounded-full">
              {allArtifacts.length}
            </span>
          )}
          <button
            onClick={refresh}
            className="ml-auto p-1 rounded hover:bg-secondary text-muted-foreground hover:text-foreground transition-colors"
            title="Refresh"
          >
            <RefreshCw className={cn("w-3.5 h-3.5", loading && "animate-spin")} />
          </button>
        </div>

        {/* Search */}
        <div className="px-3 py-2 border-b border-border shrink-0">
          <div className="flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-secondary border border-border">
            <Search className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
            <input
              className="flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground"
              placeholder="Search artifacts…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            {search && (
              <button onClick={() => setSearch("")} className="text-muted-foreground hover:text-foreground">
                <X className="w-3 h-3" />
              </button>
            )}
          </div>
        </div>

        {/* Type filter */}
        <div className="flex items-center gap-1 px-3 py-2 border-b border-border overflow-x-auto shrink-0">
          {filterButtons.map(({ key, label }) => (
            <button
              key={key}
              onClick={() => setFilter(key)}
              className={cn(
                "px-2 py-0.5 rounded-md text-[11px] whitespace-nowrap transition-colors border",
                filter === key
                  ? "bg-primary/20 text-primary border-primary/30"
                  : "text-muted-foreground border-transparent hover:text-foreground hover:bg-secondary"
              )}
            >
              {label}
            </button>
          ))}
        </div>

        {/* List */}
        <div className="flex-1 overflow-y-auto">
          {loading ? (
            <div className="flex items-center justify-center h-32 text-xs text-muted-foreground">
              Loading…
            </div>
          ) : filtered.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-32 gap-2 text-center px-4">
              <Layers className="w-8 h-8 text-muted-foreground/30" />
              <p className="text-xs text-muted-foreground">
                {allArtifacts.length === 0
                  ? "No artifacts yet.\nAsk the AI to generate code,\ndocuments or charts."
                  : "No results for your search."}
              </p>
            </div>
          ) : (
            filtered.map((a) => {
              const meta = TYPE_META[a.artifact_type as ArtifactType] ?? TYPE_META.text;
              return (
                <button
                  key={a.id}
                  onClick={() => setSelected(a)}
                  className={cn(
                    "group w-full flex items-start gap-2.5 px-3 py-2.5 text-left transition-colors border-b border-border/50",
                    selected?.id === a.id
                      ? "bg-primary/10 border-l-2 border-l-primary"
                      : "hover:bg-secondary"
                  )}
                >
                  <span className={cn("mt-0.5 shrink-0 p-1 rounded border", meta.color)}>
                    {meta.icon}
                  </span>
                  <div className="flex-1 min-w-0">
                    <p className="text-xs font-medium truncate text-foreground">{a.title}</p>
                    <p className="text-[10px] text-muted-foreground mt-0.5">
                      {meta.label}{a.language ? ` · ${a.language}` : ""} · {formatDate(a.created_at)}
                    </p>
                    <p className="text-[10px] text-muted-foreground/60 truncate mt-0.5 font-mono">
                      {a.content.slice(0, 60).replace(/\s+/g, " ")}
                    </p>
                  </div>
                </button>
              );
            })
          )}
        </div>
      </div>

      {/* ── Right: preview ─────────────────────────────────────────────── */}
      <div className="flex-1 flex flex-col h-full overflow-hidden min-w-0">
        {selected ? (
          <>
            {/* Preview header */}
            <div className="flex items-center gap-3 px-5 h-14 border-b border-border shrink-0">
              <span className={cn("p-1.5 rounded border shrink-0", typeMeta?.color)}>
                {typeMeta?.icon}
              </span>
              <div className="flex-1 min-w-0">
                <p className="text-sm font-semibold truncate">{selected.title}</p>
                <p className="text-[10px] text-muted-foreground">
                  {typeMeta?.label}
                  {selected.language ? ` · ${selected.language}` : ""}
                  {" · "}
                  {formatDate(selected.updated_at)}
                </p>
              </div>
              <div className="flex items-center gap-1 shrink-0">
                <button
                  onClick={() => handleCopy(selected)}
                  className="p-1.5 rounded hover:bg-secondary transition-colors"
                  title="Copy source"
                >
                  {copied
                    ? <Check className="w-4 h-4 text-emerald-400" />
                    : <Copy  className="w-4 h-4 text-muted-foreground" />}
                </button>
                <button
                  onClick={() => handleDownload(selected)}
                  className="p-1.5 rounded hover:bg-secondary transition-colors"
                  title="Download"
                >
                  <Download className="w-4 h-4 text-muted-foreground" />
                </button>
                <button
                  onClick={() => handleDelete(selected)}
                  className="p-1.5 rounded hover:bg-secondary transition-colors"
                  title="Delete artifact"
                >
                  <Trash2 className="w-4 h-4 text-muted-foreground hover:text-destructive" />
                </button>
              </div>
            </div>

            {/* Preview body */}
            <div className="flex-1 overflow-hidden">
              <ArtifactPreview artifact={selected} />
            </div>
          </>
        ) : (
          <div className="flex flex-col items-center justify-center h-full gap-3 text-center text-muted-foreground">
            <Layers className="w-12 h-12 opacity-20" />
            <p className="text-sm">Select an artifact to preview it</p>
          </div>
        )}
      </div>
    </div>
  );
}
