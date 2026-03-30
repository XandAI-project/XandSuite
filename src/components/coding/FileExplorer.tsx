import { useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  File,
  Folder,
  FolderOpen,
  FolderSymlink,
  RefreshCw,
} from "lucide-react";
import { useCodingStore } from "@/stores/codingStore";
import { cn } from "@/lib/utils";
import type { FileTreeEntry } from "@/lib/tauri";

// ── Language icon map ─────────────────────────────────────────────────────────

function getFileColor(name: string): string {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const map: Record<string, string> = {
    ts: "text-blue-400",
    tsx: "text-blue-300",
    js: "text-yellow-400",
    jsx: "text-yellow-300",
    rs: "text-orange-400",
    py: "text-green-400",
    go: "text-cyan-400",
    json: "text-amber-300",
    toml: "text-amber-400",
    md: "text-slate-300",
    css: "text-pink-400",
    html: "text-orange-300",
    sh: "text-emerald-400",
    yaml: "text-purple-300",
    yml: "text-purple-300",
    sql: "text-blue-300",
    lock: "text-muted-foreground",
  };
  return map[ext] ?? "text-muted-foreground";
}

// ── Tree node ─────────────────────────────────────────────────────────────────

function TreeNode({
  entry,
  depth,
  activePath,
  onSelect,
}: {
  entry: FileTreeEntry;
  depth: number;
  activePath: string | null;
  onSelect: (entry: FileTreeEntry) => void;
}) {
  const [expanded, setExpanded] = useState(depth < 1);

  if (entry.type === "directory") {
    return (
      <div>
        <button
          className={cn(
            "flex items-center gap-1 w-full text-left hover:bg-secondary/60 rounded px-1 py-0.5 text-xs transition-colors",
            "text-muted-foreground hover:text-foreground"
          )}
          style={{ paddingLeft: `${depth * 12 + 4}px` }}
          onClick={() => setExpanded((v) => !v)}
        >
          {expanded ? (
            <ChevronDown className="w-3 h-3 shrink-0 text-muted-foreground/60" />
          ) : (
            <ChevronRight className="w-3 h-3 shrink-0 text-muted-foreground/60" />
          )}
          {expanded ? (
            <FolderOpen className="w-3.5 h-3.5 shrink-0 text-amber-400/80" />
          ) : (
            <Folder className="w-3.5 h-3.5 shrink-0 text-amber-400/60" />
          )}
          <span className="truncate font-medium">{entry.name}</span>
        </button>
        {expanded && entry.children && (
          <div>
            {entry.children.map((child) => (
              <TreeNode
                key={child.path}
                entry={child}
                depth={depth + 1}
                activePath={activePath}
                onSelect={onSelect}
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  const isActive = activePath === entry.path;

  return (
    <button
      className={cn(
        "flex items-center gap-1 w-full text-left rounded px-1 py-0.5 text-xs transition-colors",
        isActive
          ? "bg-primary/15 text-foreground"
          : "text-muted-foreground hover:bg-secondary/60 hover:text-foreground"
      )}
      style={{ paddingLeft: `${depth * 12 + 20}px` }}
      onClick={() => onSelect(entry)}
    >
      <File className={cn("w-3.5 h-3.5 shrink-0", getFileColor(entry.name))} />
      <span className="truncate">{entry.name}</span>
      {entry.size !== undefined && entry.size > 0 && (
        <span className="ml-auto text-[10px] text-muted-foreground/40 shrink-0">
          {entry.size < 1024
            ? `${entry.size}B`
            : entry.size < 1024 * 1024
            ? `${(entry.size / 1024).toFixed(0)}K`
            : `${(entry.size / 1024 / 1024).toFixed(1)}M`}
        </span>
      )}
    </button>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export function FileExplorer() {
  const {
    projectPath,
    fileTree,
    fileTreeLoading,
    openFile,
    selectProject,
    loadFileTree,
    openFilePreview,
  } = useCodingStore();

  const handleSelect = (entry: FileTreeEntry) => {
    if (entry.type === "file") {
      openFilePreview(entry.path);
    }
  };

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* Header */}
      <div className="px-2 py-2 border-b border-border flex items-center gap-1 shrink-0">
        <FolderSymlink className="w-3.5 h-3.5 text-primary shrink-0" />
        <span className="text-[11px] font-semibold text-muted-foreground flex-1 truncate">
          {projectPath
            ? projectPath.split(/[\\/]/).pop()
            : "No project"}
        </span>
        <button
          title="Refresh file tree"
          onClick={loadFileTree}
          disabled={!projectPath || fileTreeLoading}
          className="p-1 rounded hover:bg-secondary transition-colors text-muted-foreground hover:text-foreground disabled:opacity-30"
        >
          <RefreshCw className={cn("w-3 h-3", fileTreeLoading && "animate-spin")} />
        </button>
      </div>

      {/* Tree or empty state */}
      <div className="flex-1 overflow-y-auto min-h-0 py-1">
        {projectPath ? (
          fileTree.length > 0 ? (
            fileTree.map((entry) => (
              <TreeNode
                key={entry.path}
                entry={entry}
                depth={0}
                activePath={openFile}
                onSelect={handleSelect}
              />
            ))
          ) : (
            <div className="flex flex-col items-center justify-center h-24 text-center">
              {fileTreeLoading ? (
                <RefreshCw className="w-4 h-4 animate-spin text-muted-foreground/40" />
              ) : (
                <p className="text-[11px] text-muted-foreground/50">Empty folder</p>
              )}
            </div>
          )
        ) : (
          <div className="flex flex-col items-center justify-center h-full gap-2 px-2 py-4">
            <Folder className="w-8 h-8 text-muted-foreground/20" />
            <p className="text-[11px] text-muted-foreground/50 text-center leading-relaxed">
              Select a project folder to browse files
            </p>
          </div>
        )}
      </div>

      {/* Select folder button */}
      <div className="px-2 py-2 border-t border-border shrink-0">
        <button
          onClick={selectProject}
          className="w-full flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-md text-[11px] border border-dashed border-border text-muted-foreground hover:border-primary/50 hover:text-foreground transition-colors"
        >
          <FolderOpen className="w-3.5 h-3.5" />
          {projectPath ? "Change folder" : "Select project folder"}
        </button>
      </div>
    </div>
  );
}
