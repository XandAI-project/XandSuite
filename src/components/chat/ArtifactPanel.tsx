import { useState, useRef, useEffect, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import {
  X, Copy, Check, Download, Pencil, Save, Trash2,
  Code, FileText, Globe, AlignLeft, FileDown, Loader2,
} from "lucide-react";
import { useArtifactStore } from "@/stores/artifactStore";
import { cn } from "@/lib/utils";
import type { Artifact, ArtifactType } from "@/lib/tauri";
import { injectExportScript, exportArtifactToPdf } from "@/lib/exportPdf";

const TYPE_META: Record<ArtifactType, { label: string; icon: React.ReactNode }> = {
  code: { label: "Code", icon: <Code className="w-3.5 h-3.5" /> },
  markdown: { label: "Document", icon: <FileText className="w-3.5 h-3.5" /> },
  html: { label: "HTML", icon: <Globe className="w-3.5 h-3.5" /> },
  text: { label: "Text", icon: <AlignLeft className="w-3.5 h-3.5" /> },
};

interface ArtifactContentProps {
  artifact: Artifact;
  iframeRef?: React.RefObject<HTMLIFrameElement | null>;
}

function ArtifactContent({ artifact, iframeRef }: ArtifactContentProps) {
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

  // text
  return (
    <pre className="p-5 text-xs font-mono whitespace-pre-wrap break-words overflow-auto h-full text-foreground">
      {artifact.content}
    </pre>
  );
}

export function ArtifactPanel() {
  const { artifacts, activeArtifactId, closePanel, openArtifact, updateArtifact, deleteArtifact, dismissArtifact } =
    useArtifactStore();

  const [copied, setCopied] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editContent, setEditContent] = useState("");
  const [exporting, setExporting] = useState(false);

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
    const ext =
      active.artifact_type === "code"
        ? active.language ?? "txt"
        : active.artifact_type === "html"
        ? "html"
        : active.artifact_type === "markdown"
        ? "md"
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
