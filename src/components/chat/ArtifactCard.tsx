import { Code, FileText, Globe, AlignLeft, Download, ExternalLink, FileDown } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import type { ArtifactType } from "@/lib/tauri";
import { useArtifactStore } from "@/stores/artifactStore";

interface ParsedArtifact {
  id: string;
  title: string;
  artifact_type: ArtifactType;
  language?: string;
  content: string;
}

interface Props {
  artifact: ParsedArtifact;
}

const TYPE_META: Record<ArtifactType, { label: string; icon: React.ReactNode; color: string }> = {
  code: {
    label: "Code",
    icon: <Code className="w-4 h-4" />,
    color: "text-blue-400 bg-blue-500/10 border-blue-500/30",
  },
  markdown: {
    label: "Document",
    icon: <FileText className="w-4 h-4" />,
    color: "text-emerald-400 bg-emerald-500/10 border-emerald-500/30",
  },
  html: {
    label: "HTML",
    icon: <Globe className="w-4 h-4" />,
    color: "text-orange-400 bg-orange-500/10 border-orange-500/30",
  },
  text: {
    label: "Text",
    icon: <AlignLeft className="w-4 h-4" />,
    color: "text-slate-400 bg-slate-500/10 border-slate-500/30",
  },
  csv: {
    label: "CSV",
    icon: <FileText className="w-4 h-4" />,
    color: "text-yellow-400 bg-yellow-500/10 border-yellow-500/30",
  },
  json: {
    label: "JSON",
    icon: <FileText className="w-4 h-4" />,
    color: "text-purple-400 bg-purple-500/10 border-purple-500/30",
  },
  pdf: {
    label: "PDF",
    icon: <FileDown className="w-4 h-4" />,
    color: "text-red-400 bg-red-500/10 border-red-500/30",
  },
};

export function ArtifactCard({ artifact }: Props) {
  const { openArtifact, artifacts } = useArtifactStore();
  const meta = TYPE_META[artifact.artifact_type] ?? TYPE_META.text;
  const isPdf = artifact.artifact_type === "pdf";

  // Parse PDF metadata from content JSON if applicable
  const pdfMeta = isPdf ? (() => {
    try { return JSON.parse(artifact.content) as { path: string; filename: string; pages: number }; }
    catch { return null; }
  })() : null;

  const handleDownload = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isPdf && pdfMeta?.path) {
      openPath(pdfMeta.path).catch(console.error);
      return;
    }
    const ext =
      artifact.artifact_type === "code"
        ? artifact.language ?? "txt"
        : artifact.artifact_type === "html"
        ? "html"
        : artifact.artifact_type === "markdown"
        ? "md"
        : "txt";
    const blob = new Blob([artifact.content], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${artifact.title.replace(/\s+/g, "_")}.${ext}`;
    a.click();
    URL.revokeObjectURL(url);
  };

  // An artifact is "saved" when its id matches a DB record (not a temp parsed- id).
  // MessageBubble already swaps the id to the DB id when it finds a match by title+type.
  const isSaved = artifacts.some((a) => a.id === artifact.id);

  const handleOpen = () => {
    if (isSaved) openArtifact(artifact.id);
  };

  return (
    <div
      className={`flex items-center gap-3 rounded-xl border px-4 py-3 mt-2 cursor-pointer
        hover:brightness-110 transition-all select-none ${meta.color}`}
      onClick={handleOpen}
      title="Click to open artifact"
    >
      <div className="shrink-0">{meta.icon}</div>

      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium truncate">{artifact.title}</p>
        <p className="text-xs opacity-60">
          {meta.label}
          {isPdf && pdfMeta?.pages ? ` · ${pdfMeta.pages} page${pdfMeta.pages !== 1 ? "s" : ""}` : ""}
          {!isPdf && artifact.language ? ` · ${artifact.language}` : ""}
        </p>
      </div>

      <div className="flex items-center gap-1 shrink-0">
        {isPdf ? (
          /* For PDFs: open the file with the system viewer */
          <button
            onClick={handleDownload}
            className="p-1.5 rounded-md hover:bg-white/10 transition-colors"
            title="Open with system viewer"
          >
            <ExternalLink className="w-3.5 h-3.5" />
          </button>
        ) : (
          <>
            <button
              onClick={handleDownload}
              className="p-1.5 rounded-md hover:bg-white/10 transition-colors"
              title="Download"
            >
              <Download className="w-3.5 h-3.5" />
            </button>
            {isSaved && (
              <button
                onClick={(e) => { e.stopPropagation(); handleOpen(); }}
                className="p-1.5 rounded-md hover:bg-white/10 transition-colors"
                title="Open in panel"
              >
                <ExternalLink className="w-3.5 h-3.5" />
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}
