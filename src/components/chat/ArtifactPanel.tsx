import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import {
  X, Copy, Check, Download, Pencil, Save, Trash2, Undo2,
  Code, FileText, Globe, AlignLeft, FileDown, Loader2, FileJson,
  ChevronDown, ChevronRight, ExternalLink, FolderOpen,
} from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import { useArtifactStore } from "@/stores/artifactStore";
import { cn } from "@/lib/utils";
import type { Artifact, ArtifactType } from "@/lib/tauri";
import { injectExportScript, exportArtifactToPdf } from "@/lib/exportPdf";

const TYPE_META: Record<ArtifactType, { label: string; icon: React.ReactNode }> = {
  code: { label: "Code", icon: <Code className="w-3.5 h-3.5" /> },
  markdown: { label: "Document", icon: <FileText className="w-3.5 h-3.5" /> },
  html: { label: "HTML", icon: <Globe className="w-3.5 h-3.5" /> },
  text: { label: "Text", icon: <AlignLeft className="w-3.5 h-3.5" /> },
  csv: { label: "CSV", icon: <FileText className="w-3.5 h-3.5" /> },
  json: { label: "JSON", icon: <FileJson className="w-3.5 h-3.5" /> },
  pdf: { label: "PDF", icon: <FileDown className="w-3.5 h-3.5" /> },
};

// ── CSV renderer ─────────────────────────────────────────────────────────────

function parseCSV(raw: string): { headers: string[]; rows: string[][] } {
  // Strip leading/trailing markdown fences if the LLM wrapped them
  const cleaned = raw.replace(/^```[\w]*\n?/, "").replace(/\n?```\s*$/, "").trim();
  const lines = cleaned.split(/\r?\n/).filter((l) => l.trim());
  if (lines.length === 0) return { headers: [], rows: [] };

  const parseLine = (line: string): string[] => {
    const result: string[] = [];
    let cur = "";
    let inQuote = false;
    for (let i = 0; i < line.length; i++) {
      const ch = line[i];
      if (ch === '"') {
        if (inQuote && line[i + 1] === '"') { cur += '"'; i++; }
        else { inQuote = !inQuote; }
      } else if (ch === "," && !inQuote) {
        result.push(cur.trim()); cur = "";
      } else {
        cur += ch;
      }
    }
    result.push(cur.trim());
    return result;
  };

  const headers = parseLine(lines[0]);
  const rows = lines.slice(1).map(parseLine);
  return { headers, rows };
}

function CsvViewer({ content }: { content: string }) {
  const [sortCol, setSortCol] = useState<number | null>(null);
  const [sortAsc, setSortAsc] = useState(true);
  const [search, setSearch] = useState("");
  const { headers, rows } = parseCSV(content);

  const handleSort = (i: number) => {
    if (sortCol === i) setSortAsc((v) => !v);
    else { setSortCol(i); setSortAsc(true); }
  };

  const filtered = rows.filter((row) =>
    !search || row.some((cell) => cell.toLowerCase().includes(search.toLowerCase()))
  );

  const sorted =
    sortCol === null
      ? filtered
      : [...filtered].sort((a, b) => {
          const av = a[sortCol] ?? "";
          const bv = b[sortCol] ?? "";
          const numA = parseFloat(av.replace(/[^0-9.-]/g, ""));
          const numB = parseFloat(bv.replace(/[^0-9.-]/g, ""));
          const cmp =
            !isNaN(numA) && !isNaN(numB)
              ? numA - numB
              : av.localeCompare(bv, undefined, { numeric: true });
          return sortAsc ? cmp : -cmp;
        });

  if (headers.length === 0) {
    return <pre className="p-5 text-xs font-mono whitespace-pre-wrap">{content}</pre>;
  }

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-3 px-4 py-2 border-b border-border shrink-0">
        <input
          className="flex-1 text-xs bg-secondary rounded-md px-3 py-1.5 outline-none placeholder:text-muted-foreground"
          placeholder="Filter rows…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <span className="text-xs text-muted-foreground whitespace-nowrap">
          {sorted.length} / {rows.length} rows
        </span>
      </div>
      {/* Table */}
      <div className="flex-1 overflow-auto">
        <table className="w-full text-xs border-collapse">
          <thead className="sticky top-0 z-10 bg-card">
            <tr>
              {headers.map((h, i) => (
                <th
                  key={i}
                  onClick={() => handleSort(i)}
                  className="px-3 py-2 text-left font-semibold border-b border-border bg-secondary/70 cursor-pointer select-none hover:bg-secondary whitespace-nowrap"
                >
                  <span className="flex items-center gap-1">
                    {h}
                    {sortCol === i ? (
                      sortAsc ? <ChevronDown className="w-3 h-3 shrink-0" /> : <ChevronRight className="w-3 h-3 shrink-0 rotate-[-90deg]" />
                    ) : (
                      <span className="w-3 h-3 opacity-0 group-hover:opacity-40">↕</span>
                    )}
                  </span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sorted.map((row, ri) => (
              <tr
                key={ri}
                className={cn(
                  "border-b border-border/50 transition-colors",
                  ri % 2 === 0 ? "bg-transparent" : "bg-secondary/20",
                  "hover:bg-primary/5"
                )}
              >
                {headers.map((_, ci) => (
                  <td key={ci} className="px-3 py-1.5 text-foreground/80">
                    {row[ci] ?? ""}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        {sorted.length === 0 && (
          <div className="p-8 text-center text-xs text-muted-foreground">No rows match your filter.</div>
        )}
      </div>
    </div>
  );
}

// ── JSON renderer ─────────────────────────────────────────────────────────────

function JsonNode({ data, depth = 0 }: { data: unknown; depth?: number }) {
  const [open, setOpen] = useState(depth < 2);

  if (data === null) return <span className="text-orange-400/80">null</span>;
  if (typeof data === "boolean") return <span className="text-blue-400">{String(data)}</span>;
  if (typeof data === "number") return <span className="text-emerald-400">{String(data)}</span>;
  if (typeof data === "string") return <span className="text-amber-300/90">"{data}"</span>;

  if (Array.isArray(data)) {
    if (data.length === 0) return <span className="text-muted-foreground">[]</span>;
    return (
      <span>
        <button onClick={() => setOpen((v) => !v)} className="text-muted-foreground hover:text-foreground">
          {open ? "▾" : "▸"} <span className="text-muted-foreground">[{data.length}]</span>
        </button>
        {open && (
          <div className="pl-4 border-l border-border/30 mt-0.5">
            {data.map((item, i) => (
              <div key={i} className="flex gap-1.5 py-0.5">
                <span className="text-muted-foreground/50 text-[10px] w-5 text-right shrink-0">{i}</span>
                <JsonNode data={item} depth={depth + 1} />
              </div>
            ))}
          </div>
        )}
      </span>
    );
  }

  if (typeof data === "object" && data !== null) {
    const entries = Object.entries(data as Record<string, unknown>);
    if (entries.length === 0) return <span className="text-muted-foreground">{"{}"}</span>;
    return (
      <span>
        <button onClick={() => setOpen((v) => !v)} className="text-muted-foreground hover:text-foreground">
          {open ? "▾" : "▸"} <span className="text-muted-foreground">{"{" + entries.length + "}"}</span>
        </button>
        {open && (
          <div className="pl-4 border-l border-border/30 mt-0.5">
            {entries.map(([key, val]) => (
              <div key={key} className="flex gap-1.5 py-0.5 flex-wrap">
                <span className="text-violet-400/80 shrink-0">"{key}"</span>
                <span className="text-muted-foreground/50">:</span>
                <JsonNode data={val} depth={depth + 1} />
              </div>
            ))}
          </div>
        )}
      </span>
    );
  }

  return <span>{String(data)}</span>;
}

function JsonViewer({ content }: { content: string }) {
  const cleaned = content.replace(/^```[\w]*\n?/, "").replace(/\n?```\s*$/, "").trim();
  let parsed: unknown;
  try {
    parsed = JSON.parse(cleaned);
  } catch {
    // Fall back to syntax-highlighted raw text
    return (
      <SyntaxHighlighter
        style={vscDarkPlus as Record<string, React.CSSProperties>}
        language="json"
        PreTag="div"
        className="!rounded-none !m-0 !text-xs !h-full !overflow-auto"
        showLineNumbers
      >
        {content}
      </SyntaxHighlighter>
    );
  }
  return (
    <div className="p-4 text-xs font-mono overflow-auto h-full leading-relaxed">
      <JsonNode data={parsed} depth={0} />
    </div>
  );
}

// ── PDF viewer ────────────────────────────────────────────────────────────────

function PdfViewer({ content, title }: { content: string; title: string }) {
  const [dataUri, setDataUri] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const meta = (() => {
    try { return JSON.parse(content) as { path: string; filename: string; pages: number }; }
    catch { return null; }
  })();

  const path = meta?.path ?? "";
  const filename = meta?.filename ?? title;
  const pages = meta?.pages ?? 0;

  // Load PDF bytes from disk as base64 and build a data URI for the iframe
  useEffect(() => {
    if (!path) return;
    setLoading(true);
    setError(null);
    invoke<string>("read_file_as_base64", { path })
      .then((b64) => setDataUri(`data:application/pdf;base64,${b64}`))
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [path]);

  const openFolder = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!path) return;
    const dir = path.replace(/[\\/][^\\/]+$/, "");
    openPath(dir).catch(console.error);
  };

  const openFile = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!path) return;
    openPath(path).catch(console.error);
  };

  return (
    <div className="flex flex-col h-full">
      {/* Thin toolbar above the PDF */}
      <div className="flex items-center gap-2 px-3 py-1.5 border-b border-border bg-card shrink-0">
        <FileDown className="w-3.5 h-3.5 text-red-400 shrink-0" />
        <span className="text-xs font-medium truncate flex-1 text-foreground">{filename}</span>
        {pages > 0 && (
          <span className="text-[10px] text-muted-foreground shrink-0">{pages}p</span>
        )}
        <button
          onClick={openFile}
          className="flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground
                     px-2 py-1 rounded hover:bg-secondary transition-colors shrink-0"
          title="Open with system viewer"
        >
          <ExternalLink className="w-3 h-3" />
          Open
        </button>
        <button
          onClick={openFolder}
          className="flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground
                     px-2 py-1 rounded hover:bg-secondary transition-colors shrink-0"
          title="Show in folder"
        >
          <FolderOpen className="w-3 h-3" />
          Folder
        </button>
      </div>

      {/* PDF content area */}
      <div className="flex-1 overflow-hidden bg-[#525659]">
        {loading && (
          <div className="flex flex-col items-center justify-center h-full gap-3">
            <Loader2 className="w-6 h-6 text-muted-foreground animate-spin" />
            <p className="text-xs text-muted-foreground">Loading PDF…</p>
          </div>
        )}
        {error && (
          <div className="flex flex-col items-center justify-center h-full gap-3 p-6">
            <FileDown className="w-8 h-8 text-red-400/60" />
            <p className="text-xs text-muted-foreground text-center">{error}</p>
            <button
              onClick={openFile}
              className="flex items-center gap-2 px-3 py-1.5 bg-red-500/20 hover:bg-red-500/30
                         border border-red-500/30 text-red-300 rounded-lg text-xs transition-colors"
            >
              <ExternalLink className="w-3.5 h-3.5" />
              Open with system viewer
            </button>
          </div>
        )}
        {!loading && !error && dataUri && (
          <iframe
            src={dataUri}
            className="w-full h-full border-none"
            title={filename}
          />
        )}
      </div>
    </div>
  );
}

// ── ArtifactContent ────────────────────────────────────────────────────────────

interface ArtifactContentProps {
  artifact: Artifact;
  iframeRef?: React.RefObject<HTMLIFrameElement | null>;
}

function ArtifactContent({ artifact, iframeRef }: ArtifactContentProps) {
  const type = artifact.artifact_type as ArtifactType;

  if (type === "pdf") {
    return <PdfViewer content={artifact.content} title={artifact.title} />;
  }

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
        ref={iframeRef}
        srcDoc={injectExportScript(artifact.content)}
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
                <code className="px-1 py-0.5 rounded bg-muted text-xs font-mono">
                  {children}
                </code>
              );
            },
            table: ({ children }) => (
              <div className="overflow-x-auto my-3">
                <table className="border-collapse w-full text-xs">{children}</table>
              </div>
            ),
            th: ({ children }) => (
              <th className="border border-border px-3 py-1.5 bg-secondary text-left font-semibold">
                {children}
              </th>
            ),
            td: ({ children }) => (
              <td className="border border-border px-3 py-1.5">{children}</td>
            ),
          }}
        >
          {artifact.content}
        </ReactMarkdown>
      </div>
    );
  }

  if (type === "csv") {
    return <CsvViewer content={artifact.content} />;
  }

  if (type === "json") {
    return <JsonViewer content={artifact.content} />;
  }

  // text
  return (
    <pre className="p-5 text-xs font-mono whitespace-pre-wrap break-words overflow-auto h-full text-foreground">
      {artifact.content}
    </pre>
  );
}

export function ArtifactPanel() {
  const { artifacts, activeArtifactId, closePanel, openArtifact, updateArtifact, deleteArtifact, dismissArtifact, fetchArtifacts, aiEdited } =
    useArtifactStore();

  const [copied, setCopied] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editContent, setEditContent] = useState("");
  const [exporting, setExporting] = useState(false);
  const [undoing, setUndoing] = useState(false);

  const iframeRef = useRef<HTMLIFrameElement | null>(null);

  const active = artifacts.find((a) => a.id === activeArtifactId) ?? artifacts[0] ?? null;
  const meta = active ? (TYPE_META[active.artifact_type as ArtifactType] ?? TYPE_META.text) : null;

  // Listen for canvas data postMessage from the HTML iframe
  const handleExportMessage = useCallback(
    (e: MessageEvent) => {
      if (!e.data || e.data.type !== "xandsuite-export") return;
      if (!active) return;
      exportArtifactToPdf(active, e.data.images as string[]).finally(() =>
        setExporting(false)
      );
    },
    [active],
  );

  useEffect(() => {
    window.addEventListener("message", handleExportMessage);
    return () => window.removeEventListener("message", handleExportMessage);
  }, [handleExportMessage]);

  const handleUndoAiEdit = async () => {
    if (!active || undoing) return;
    setUndoing(true);
    try {
      await invoke("undo_artifact_edit", { id: active.id });
      const convId = active.conversation_id;
      if (convId) {
        await fetchArtifacts(convId);
      }
    } catch (e) {
      console.error("Undo failed:", e);
    } finally {
      setUndoing(false);
    }
  };

  const handleExportPdf = () => {
    if (!active || exporting) return;
    if (active.artifact_type === "html") {
      setExporting(true);
      iframeRef.current?.contentWindow?.postMessage(
        { type: "xandsuite-export-request" },
        "*",
      );
      // Safety timeout — if iframe doesn't respond (no canvas) fall through
      setTimeout(() => setExporting((v) => { if (v) exportArtifactToPdf(active, []).finally(() => {}); return false; }), 3000);
    } else {
      exportArtifactToPdf(active);
    }
  };

  const handleCopy = () => {
    if (!active) return;
    navigator.clipboard.writeText(active.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDownload = () => {
    if (!active) return;
    // PDFs are already on disk — open them with the system viewer instead
    if (active.artifact_type === "pdf") {
      try {
        const meta = JSON.parse(active.content) as { path?: string };
        if (meta.path) openPath(meta.path).catch(console.error);
      } catch { /* ignore parse errors */ }
      return;
    }
    const ext =
      active.artifact_type === "code"
        ? active.language ?? "txt"
        : active.artifact_type === "html"
        ? "html"
        : active.artifact_type === "markdown"
        ? "md"
        : active.artifact_type === "csv"
        ? "csv"
        : active.artifact_type === "json"
        ? "json"
        : "txt";
    const blob = new Blob([active.content], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${active.title.replace(/\s+/g, "_")}.${ext}`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const startEditing = () => {
    if (!active) return;
    setEditTitle(active.title);
    setEditContent(active.content);
    setEditing(true);
  };

  const saveEdit = async () => {
    if (!active) return;
    await updateArtifact(active.id, editTitle, editContent);
    setEditing(false);
  };

  const handleDelete = async () => {
    if (!active) return;
    await deleteArtifact(active.id);
    setEditing(false);
  };

  if (!active) return null;

  return (
    <div className="flex flex-col h-full border-l border-border bg-card">
      {/* Artifact tab strip — always visible so tabs can be closed */}
      <div className="flex items-center gap-0.5 px-2 py-1.5 border-b border-border overflow-x-auto shrink-0 min-h-[36px]">
        {artifacts.map((a) => (
          <div
            key={a.id}
            className={cn(
              "group/tab flex items-center gap-1.5 pl-2.5 pr-1 py-1 rounded-md text-xs whitespace-nowrap transition-colors",
              a.id === active.id
                ? "bg-primary/20 text-primary border border-primary/30"
                : "text-muted-foreground hover:text-foreground hover:bg-secondary border border-transparent"
            )}
          >
            {/* Tab label — click to switch */}
            <button
              onClick={() => { openArtifact(a.id); setEditing(false); }}
              className="flex items-center gap-1.5 min-w-0"
            >
              {TYPE_META[a.artifact_type as ArtifactType]?.icon}
              <span className="max-w-[110px] truncate">{a.title}</span>
            </button>

            {/* Close tab — hides from panel only, does NOT delete from DB */}
            <button
              onClick={(e) => {
                e.stopPropagation();
                dismissArtifact(a.id);
              }}
              className={cn(
                "ml-0.5 rounded p-0.5 transition-colors shrink-0",
                a.id === active.id
                  ? "text-primary/50 hover:text-primary hover:bg-primary/20"
                  : "text-transparent group-hover/tab:text-muted-foreground hover:!text-foreground hover:bg-secondary"
              )}
              title="Close tab"
            >
              <X className="w-3 h-3" />
            </button>
          </div>
        ))}
      </div>

      {/* Panel header */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border shrink-0">
        <div className="flex items-center gap-2 flex-1 min-w-0">
          <span className="text-muted-foreground shrink-0">{meta?.icon}</span>
          {editing ? (
            <input
              className="flex-1 bg-transparent text-sm font-medium outline-none border-b border-primary"
              value={editTitle}
              onChange={(e) => setEditTitle(e.target.value)}
              autoFocus
            />
          ) : (
            <span className="text-sm font-medium truncate">{active.title}</span>
          )}
          <span className="text-[10px] text-muted-foreground bg-secondary px-1.5 py-0.5 rounded shrink-0">
            {meta?.label}
            {active.language ? ` · ${active.language}` : ""}
          </span>
          {aiEdited && (
            <span className="text-[10px] text-primary bg-primary/10 border border-primary/30 px-1.5 py-0.5 rounded shrink-0 animate-pulse">
              Edited by AI
            </span>
          )}
        </div>

        <div className="flex items-center gap-0.5 shrink-0">
          {editing ? (
            <>
              <button
                onClick={saveEdit}
                className="p-1.5 rounded hover:bg-secondary transition-colors text-emerald-400"
                title="Save changes"
              >
                <Save className="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => setEditing(false)}
                className="p-1.5 rounded hover:bg-secondary transition-colors text-muted-foreground"
                title="Cancel"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </>
          ) : (
            <>
              {active.artifact_type !== "pdf" && (
                <>
                  <button onClick={handleCopy} className="p-1.5 rounded hover:bg-secondary transition-colors" title="Copy source">
                    {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5 text-muted-foreground" />}
                  </button>
                  <button
                    onClick={handleExportPdf}
                    className="p-1.5 rounded hover:bg-secondary transition-colors"
                    title="Export as PDF"
                    disabled={exporting}
                  >
                    {exporting
                      ? <Loader2 className="w-3.5 h-3.5 text-muted-foreground animate-spin" />
                      : <FileDown className="w-3.5 h-3.5 text-muted-foreground" />}
                  </button>
                  <button onClick={handleDownload} className="p-1.5 rounded hover:bg-secondary transition-colors" title="Download source">
                    <Download className="w-3.5 h-3.5 text-muted-foreground" />
                  </button>
                  <button onClick={startEditing} className="p-1.5 rounded hover:bg-secondary transition-colors" title="Edit">
                    <Pencil className="w-3.5 h-3.5 text-muted-foreground" />
                  </button>
                </>
              )}
              <button
                onClick={handleUndoAiEdit}
                className="p-1.5 rounded hover:bg-secondary transition-colors"
                title="Undo last AI edit"
                disabled={undoing}
              >
                {undoing
                  ? <Loader2 className="w-3.5 h-3.5 text-muted-foreground animate-spin" />
                  : <Undo2 className="w-3.5 h-3.5 text-muted-foreground" />}
              </button>
              <button onClick={handleDelete} className="p-1.5 rounded hover:bg-secondary transition-colors" title="Delete artifact">
                <Trash2 className="w-3.5 h-3.5 text-muted-foreground hover:text-destructive" />
              </button>
            </>
          )}
          <button onClick={closePanel} className="p-1.5 rounded hover:bg-secondary transition-colors ml-1" title="Close panel">
            <X className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-hidden">
        {editing ? (
          <textarea
            className="w-full h-full p-4 bg-transparent text-xs font-mono resize-none outline-none"
            value={editContent}
            onChange={(e) => setEditContent(e.target.value)}
          />
        ) : (
          <ArtifactContent artifact={active} iframeRef={iframeRef} />
        )}
      </div>
    </div>
  );
}
