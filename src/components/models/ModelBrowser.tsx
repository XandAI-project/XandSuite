import { useEffect, useRef, useState } from "react";
import {
  Search, Download, Trash2, Play, Square, RefreshCw,
  HardDrive, ChevronDown, ChevronUp, Loader2,
  CheckCircle2, ExternalLink, Cpu,
} from "lucide-react";
import { useModelStore } from "@/stores/modelStore";
import { useServerStore } from "@/stores/serverStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { GgufFile, HfModel } from "@/lib/tauri";

// ── Helpers ───────────────────────────────────────────────────────────────────

function fmtBytes(b: number): string {
  if (b >= 1e9) return `${(b / 1e9).toFixed(1)} GB`;
  if (b >= 1e6) return `${(b / 1e6).toFixed(0)} MB`;
  return `${(b / 1e3).toFixed(0)} KB`;
}

function fmtCount(n: number | null): string {
  if (n == null) return "—";
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

// Colour-code quantization levels
function quantBadgeColor(q: string | null): string {
  if (!q) return "bg-muted text-muted-foreground";
  const u = q.toUpperCase();
  if (u.includes("Q2")) return "bg-red-500/15 text-red-400 border-red-500/20";
  if (u.includes("Q3")) return "bg-orange-500/15 text-orange-400 border-orange-500/20";
  if (u.includes("Q4")) return "bg-yellow-500/15 text-yellow-400 border-yellow-500/20";
  if (u.includes("Q5")) return "bg-lime-500/15 text-lime-400 border-lime-500/20";
  if (u.includes("Q6") || u.includes("Q8")) return "bg-emerald-500/15 text-emerald-400 border-emerald-500/20";
  if (u.includes("F16") || u.includes("F32") || u.includes("BF16"))
    return "bg-blue-500/15 text-blue-400 border-blue-500/20";
  return "bg-muted text-muted-foreground border-border";
}

// ── File row inside expanded model card ──────────────────────────────────────

function GgufRow({
  file,
  isDownloaded,
  progress,
  remoteMode,
  onDownload,
  onLoad,
}: {
  file: GgufFile;
  isDownloaded: boolean;
  progress: number | null; // 0–100 while downloading, null otherwise
  /** When true, the engine is in remote mode — local Load is disabled. */
  remoteMode?: boolean;
  onDownload: (file: GgufFile) => void;
  onLoad: (filename: string) => void;
}) {
  const isDownloading = progress !== null;

  return (
    <div className="flex items-center gap-3 py-2 px-3 rounded-lg hover:bg-muted/40 transition-colors group">
      {/* Quantization badge */}
      <span className={cn(
        "text-[10px] font-mono font-semibold px-1.5 py-0.5 rounded border shrink-0",
        quantBadgeColor(file.quantization),
      )}>
        {file.quantization ?? "GGUF"}
      </span>

      {/* Filename + size */}
      <div className="flex-1 min-w-0">
        <p className="text-xs font-mono text-foreground/80 truncate">{file.filename}</p>
        {file.size_bytes && (
          <p className="text-[10px] text-muted-foreground">{fmtBytes(file.size_bytes)}</p>
        )}
      </div>

      {/* Progress bar while downloading */}
      {isDownloading && (
        <div className="w-28 shrink-0 space-y-0.5">
          <Progress value={progress ?? 0} className="h-1" />
          <p className="text-[9px] text-muted-foreground text-right">{Math.round(progress ?? 0)}%</p>
        </div>
      )}

      {/* Action buttons */}
      {isDownloaded ? (
        <Button
          size="sm"
          variant="ghost"
          className="h-7 gap-1 text-emerald-400 hover:text-emerald-300 hover:bg-emerald-500/10 shrink-0"
          onClick={() => onLoad(file.filename)}
          disabled={remoteMode}
          title={
            remoteMode
              ? "Disabled in remote engine mode — switch to Local in Settings to load models"
              : "Load this model into the engine"
          }
        >
          <Play className="w-3 h-3 fill-current" />
          Load
        </Button>
      ) : isDownloading ? (
        <Button size="sm" variant="ghost" disabled className="h-7 shrink-0">
          <Loader2 className="w-3 h-3 animate-spin" />
        </Button>
      ) : (
        <Button
          size="sm"
          variant="ghost"
          className="h-7 gap-1 opacity-0 group-hover:opacity-100 shrink-0"
          onClick={() => onDownload(file)}
          title="Download this file"
        >
          <Download className="w-3 h-3" />
          Download
        </Button>
      )}
    </div>
  );
}

// ── HF model card ─────────────────────────────────────────────────────────────

function ModelCard({
  model,
  downloadedFiles,
  allDownloaded,
  downloads,
  remoteMode,
  onDownload,
  onLoad,
}: {
  model: HfModel;
  downloadedFiles: string[];
  allDownloaded: { model_id: string; filename: string; path: string; size_bytes: number }[];
  downloads: Record<string, number | null>;
  remoteMode?: boolean;
  onDownload: (modelId: string, file: GgufFile) => void;
  onLoad: (path: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const hasFiles = model.gguf_files.length > 0;

  return (
    <div className={cn(
      "rounded-xl border border-border bg-card transition-colors",
      expanded && "border-border/80 bg-card/80",
    )}>
      {/* Card header */}
      <button
        className="w-full text-left px-4 py-3 flex items-start gap-3"
        onClick={() => hasFiles && setExpanded((v) => !v)}
        disabled={!hasFiles}
      >
        {/* Model icon placeholder */}
        <div className="w-8 h-8 rounded-lg bg-primary/10 flex items-center justify-center shrink-0 mt-0.5">
          <Cpu className="w-4 h-4 text-primary/60" />
        </div>

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="text-sm font-semibold text-foreground truncate max-w-[280px]">
              {model.name || model.id.split("/").pop()}
            </span>
            {model.is_downloaded && (
              <span className="flex items-center gap-0.5 text-[10px] text-emerald-400">
                <CheckCircle2 className="w-3 h-3" /> Downloaded
              </span>
            )}
          </div>
          <p className="text-xs text-muted-foreground mt-0.5">{model.author}</p>
          {model.description && (
            <p className="text-xs text-muted-foreground mt-1 line-clamp-2">{model.description}</p>
          )}
          <div className="flex items-center gap-3 mt-1.5 flex-wrap">
            <span className="text-[10px] text-muted-foreground">
              ↓ {fmtCount(model.downloads)} downloads
            </span>
            <span className="text-[10px] text-muted-foreground">
              ♥ {fmtCount(model.likes)}
            </span>
            {model.tags.slice(0, 4).map((tag) => (
              <Badge key={tag} variant="secondary" className="text-[10px] py-0 px-1.5 h-4">
                {tag}
              </Badge>
            ))}
          </div>
        </div>

        {hasFiles && (
          <div className="shrink-0 mt-1">
            {expanded
              ? <ChevronUp className="w-4 h-4 text-muted-foreground" />
              : <ChevronDown className="w-4 h-4 text-muted-foreground" />}
          </div>
        )}
      </button>

      {/* Expanded file list */}
      {expanded && hasFiles && (
        <div className="border-t border-border px-2 pb-2">
          {model.gguf_files.map((f) => {
            const key = `${model.id}::${f.filename}`;
            const prog = downloads[key] ?? null;
            const downloaded = downloadedFiles.includes(f.filename);
            return (
              <GgufRow
                key={f.filename}
                file={f}
                isDownloaded={downloaded}
                progress={prog}
                remoteMode={remoteMode}
                onDownload={(file) => onDownload(model.id, file)}
                onLoad={(filename) => {
                  // Resolve the full absolute path from allDownloaded list;
                  // model.local_path is null from the HF scraper, so we can't use it.
                  const entry = allDownloaded.find(
                    (d: { filename: string; path: string }) => d.filename === filename
                  );
                  onLoad(entry?.path ?? filename);
                }}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}

// ── Downloaded model row ──────────────────────────────────────────────────────

function DownloadedRow({
  model,
  isActive,
  isLoading,
  isStopping,
  remoteMode,
  mmprojPath,
  onLoad,
  onStop,
  onDelete,
}: {
  model: { model_id: string; filename: string; path: string; size_bytes: number };
  isActive: boolean;
  isLoading: boolean;
  isStopping: boolean;
  /** When true, the engine is in remote mode — local Load is disabled. */
  remoteMode?: boolean;
  /** Path to the companion mmproj file, if one exists in the same folder */
  mmprojPath?: string;
  onLoad: (modelPath: string, mmprojPath?: string) => void;
  onStop: () => void;
  onDelete: (modelId: string, filename: string) => void;
}) {
  const [confirmDelete, setConfirmDelete] = useState(false);

  return (
    <div className={cn(
      "flex items-center gap-3 px-4 py-2.5 rounded-xl border transition-colors",
      isActive
        ? "border-emerald-500/30 bg-emerald-500/5"
        : "border-border bg-card hover:bg-muted/30",
    )}>
      <HardDrive className={cn("w-4 h-4 shrink-0", isActive ? "text-emerald-400" : "text-muted-foreground")} />

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <p className="text-sm font-mono truncate text-foreground/90">{model.filename}</p>
          {mmprojPath && (
            <span className="shrink-0 text-[9px] font-semibold px-1.5 py-0.5 rounded bg-violet-500/15 text-violet-400 border border-violet-500/20 uppercase tracking-wide">
              Vision
            </span>
          )}
        </div>
        <p className="text-[10px] text-muted-foreground">{model.model_id} · {fmtBytes(model.size_bytes)}</p>
      </div>

      <div className="flex items-center gap-1 shrink-0">
        {isActive ? (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 gap-1 text-xs text-red-400 hover:text-red-300 hover:bg-red-500/10"
            onClick={onStop}
            disabled={isStopping}
            title="Stop llama-server"
          >
            {isStopping ? (
              <Loader2 className="w-3 h-3 animate-spin" />
            ) : (
              <Square className="w-3 h-3 fill-current" />
            )}
            {isStopping ? "Stopping…" : "Stop"}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 gap-1 text-xs"
            onClick={() => onLoad(model.path, mmprojPath)}
            disabled={isLoading || remoteMode}
            title={
              remoteMode
                ? "Disabled in remote engine mode — switch to Local in Settings to load models"
                : mmprojPath
                  ? "Start llama-server with vision support"
                  : "Start llama-server with this model"
            }
          >
            {isLoading ? (
              <Loader2 className="w-3 h-3 animate-spin" />
            ) : (
              <Play className="w-3 h-3 fill-current" />
            )}
            {isLoading ? "Starting…" : "Load"}
          </Button>
        )}

        {confirmDelete ? (
          <>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-xs text-destructive hover:bg-destructive/10"
              onClick={() => { onDelete(model.model_id, model.filename); setConfirmDelete(false); }}
            >
              Confirm
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-xs"
              onClick={() => setConfirmDelete(false)}
            >
              Cancel
            </Button>
          </>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive hover:bg-destructive/10"
            onClick={() => setConfirmDelete(true)}
            title="Delete from disk"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </Button>
        )}
      </div>
    </div>
  );
}

// ── Main view ─────────────────────────────────────────────────────────────────

export function ModelBrowser() {
  const {
    models, downloadedModels, downloads,
    isLoading, error,
    fetchModels, fetchDownloadedModels,
    downloadModel, deleteModel,
    listenToDownloads,
    refreshModels,
  } = useModelStore();
  const {
    status: serverStatus,
    isStarting,
    isStopping,
    error: serverError,
    engineMode,
    startServer,
    stopServer,
    fetchStatus,
  } = useServerStore();

  // In remote mode the local llama-server is not used — model loading is
  // handled by the external server, so Load/Stop controls are disabled.
  const isRemote = engineMode === "remote";

  const [search, setSearch] = useState("");
  const [tab, setTab] = useState<"browse" | "local">("local");
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Build per-file download progress 0-100
  const fileProgress: Record<string, number | null> = {};
  for (const [key, prog] of Object.entries(downloads)) {
    if (!prog) continue;
    if (prog.status === "downloading" && prog.total_bytes) {
      fileProgress[key] = (prog.downloaded_bytes / prog.total_bytes) * 100;
    } else if (prog.status === "pending") {
      fileProgress[key] = 0;
    }
  }

  // Separate mmproj files from actual model files.
  // mmproj files are vision projection weights, not standalone models.
  const isMmproj = (filename: string) =>
    filename.toLowerCase().includes("mmproj");

  const mmprojFiles = downloadedModels.filter((d) => isMmproj(d.filename));
  const actualModels = downloadedModels.filter((d) => !isMmproj(d.filename));

  // Build a map from model_id → mmproj path so the companion model row can
  // receive the right path and pass it to start_local_server.
  const mmprojByModelId: Record<string, string> = {};
  for (const mp of mmprojFiles) {
    mmprojByModelId[mp.model_id] = mp.path;
  }

  // Build per-model downloaded filenames map (for Browse tab)
  const downloadedByModel: Record<string, string[]> = {};
  for (const d of downloadedModels) {
    if (!downloadedByModel[d.model_id]) downloadedByModel[d.model_id] = [];
    downloadedByModel[d.model_id].push(d.filename);
  }

  useEffect(() => {
    fetchDownloadedModels();
    fetchStatus();
    const unlistenPromise = listenToDownloads();
    return () => {
      unlistenPromise.then((fn) => fn());
      if (searchTimer.current) clearTimeout(searchTimer.current);
    };
  }, []);

  const handleSearch = (q: string) => {
    setSearch(q);
    if (searchTimer.current) clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => fetchModels(q || undefined), 400);
  };

  const handleBrowseTab = () => {
    setTab("browse");
    if (models.length === 0) fetchModels();
  };

  const handleDownload = async (modelId: string, file: GgufFile) => {
    await downloadModel(modelId, file.filename, file.url);
  };

  const handleLoad = async (modelPath: string, mmprojPath?: string) => {
    await startServer(modelPath, mmprojPath);
    await fetchStatus();
  };

  const handleStop = async () => {
    await stopServer();
  };

  const handleDelete = async (modelId: string, filename: string) => {
    await deleteModel(modelId, filename);
  };

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-6 py-4 border-b border-border shrink-0">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-semibold">Models</h1>
            <p className="text-sm text-muted-foreground mt-0.5">
              Manage local GGUF models or browse HuggingFace
            </p>
          </div>
          <div className="flex items-center gap-2">
            {isRemote ? (
              <span className="flex items-center gap-1.5 text-xs text-blue-400">
                <ExternalLink className="w-3.5 h-3.5" />
                Remote engine mode
              </span>
            ) : serverStatus.running && (
              <span className="flex items-center gap-1.5 text-xs text-emerald-400">
                <CheckCircle2 className="w-3.5 h-3.5" />
                Engine running
              </span>
            )}
          </div>
        </div>

        {/* Tab bar */}
        <div className="flex gap-1 mt-4">
          {([
            { id: "local",  label: `Downloaded (${actualModels.length})` },
            { id: "browse", label: "Browse HuggingFace" },
          ] as const).map(({ id, label }) => (
            <button
              key={id}
              onClick={() => id === "browse" ? handleBrowseTab() : setTab("local")}
              className={cn(
                "px-3 py-1.5 rounded-lg text-sm font-medium transition-colors",
                tab === id
                  ? "bg-primary/15 text-primary"
                  : "text-muted-foreground hover:text-foreground hover:bg-muted/50",
              )}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {/* ── Local tab ── */}
      {tab === "local" && (
        <ScrollArea className="flex-1 px-6 py-4">
          {isRemote && (
            <div className="mb-4 flex items-start gap-2 rounded-lg border border-blue-500/20 bg-blue-500/5 px-4 py-2.5 text-sm max-w-2xl">
              <ExternalLink className="w-4 h-4 shrink-0 mt-0.5 text-blue-400" />
              <div className="text-xs text-muted-foreground">
                <span className="text-blue-400 font-medium">Remote engine mode is active.</span>{" "}
                Model loading is handled by the remote server configured in Settings → Remote.
                You can still download models here for later local use, but loading is disabled.
              </div>
            </div>
          )}
          {isStarting && (
            <div className="mb-4 flex items-center gap-2 rounded-lg border border-primary/20 bg-primary/5 px-4 py-2 text-sm text-primary max-w-2xl">
              <Loader2 className="w-4 h-4 animate-spin shrink-0" />
              Starting llama-server… this may take up to a minute while the model loads.
            </div>
          )}
          {serverError && !isStarting && (
            <div className="mb-4 rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-2 max-w-2xl">
              <p className="text-xs text-destructive whitespace-pre-wrap">{serverError}</p>
            </div>
          )}
          {actualModels.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-64 text-center gap-3">
              <HardDrive className="w-10 h-10 text-muted-foreground/30" />
              <p className="text-muted-foreground text-sm">No models downloaded yet.</p>
              <Button variant="outline" size="sm" onClick={handleBrowseTab}>
                Browse HuggingFace
              </Button>
            </div>
          ) : (
            <div className="space-y-2 max-w-2xl">
              {actualModels.map((m) => (
                <DownloadedRow
                  key={`${m.model_id}::${m.filename}`}
                  model={m}
                  isActive={serverStatus.running && serverStatus.model === m.path}
                  isLoading={isStarting}
                  isStopping={isStopping}
                  remoteMode={isRemote}
                  mmprojPath={mmprojByModelId[m.model_id]}
                  onLoad={handleLoad}
                  onStop={handleStop}
                  onDelete={handleDelete}
                />
              ))}
              {mmprojFiles.length > 0 && (
                <div className="mt-4 rounded-xl border border-violet-500/20 bg-violet-500/5 px-4 py-3">
                  <p className="text-xs font-semibold text-violet-400 mb-1.5">
                    Vision projection files
                  </p>
                  <div className="space-y-1">
                    {mmprojFiles.map((mp) => (
                      <div key={`${mp.model_id}::${mp.filename}`} className="flex items-center gap-2">
                        <span className="text-xs font-mono text-muted-foreground truncate flex-1">
                          {mp.filename}
                        </span>
                        <span className="text-[10px] text-muted-foreground shrink-0">
                          {fmtBytes(mp.size_bytes)} · auto-attached to {mp.model_id}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </ScrollArea>
      )}

      {/* ── Browse tab ── */}
      {tab === "browse" && (
        <>
          {/* Search bar */}
          <div className="px-6 py-3 border-b border-border shrink-0">
            <div className="flex items-center gap-2 max-w-xl">
              <div className="relative flex-1">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
                <Input
                  placeholder="Search models (e.g. llama, mistral, qwen…)"
                  className="pl-9"
                  value={search}
                  onChange={(e) => handleSearch(e.target.value)}
                />
              </div>
              <Button
                variant="outline"
                size="icon"
                onClick={() => refreshModels()}
                disabled={isLoading}
                title="Refresh model list"
              >
                <RefreshCw className={cn("w-4 h-4", isLoading && "animate-spin")} />
              </Button>
              <a
                href="https://huggingface.co/models?library=gguf&sort=downloads"
                target="_blank"
                rel="noopener noreferrer"
                title="Open HuggingFace in browser"
              >
                <Button variant="outline" size="icon">
                  <ExternalLink className="w-4 h-4" />
                </Button>
              </a>
            </div>
          </div>

          <ScrollArea className="flex-1 px-6 py-4">
            {error && (
              <div className="mb-4 rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-2">
                <p className="text-xs text-destructive">{error}</p>
              </div>
            )}

            {isLoading && models.length === 0 ? (
              <div className="flex items-center justify-center h-64 gap-2 text-muted-foreground">
                <Loader2 className="w-5 h-5 animate-spin" />
                <span className="text-sm">Loading models…</span>
              </div>
            ) : models.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-64 text-center gap-3">
                <Search className="w-10 h-10 text-muted-foreground/30" />
                <p className="text-muted-foreground text-sm">
                  No results. Try a different search term.
                </p>
              </div>
            ) : (
              <div className="space-y-2 max-w-2xl">
                {models.map((m) => (
                  <ModelCard
                    key={m.id}
                    model={m}
                    downloadedFiles={downloadedByModel[m.id] ?? []}
                    allDownloaded={downloadedModels}
                    downloads={fileProgress}
                    remoteMode={isRemote}
                    onDownload={handleDownload}
                    onLoad={handleLoad}
                  />
                ))}
              </div>
            )}
          </ScrollArea>
        </>
      )}
    </div>
  );
}
