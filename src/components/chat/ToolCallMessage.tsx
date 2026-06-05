import { useState, useEffect, useRef, memo } from "react";
import {
  ChevronDown,
  ChevronRight,
  Wrench,
  CheckCircle2,
  Loader2,
  AlertCircle,
  Terminal,
  Clock,
  ImageIcon,
  Video,
  Globe,
  MousePointerClick,
  Keyboard,
  Camera,
  FileSearch,
  ArrowLeftCircle,
  RotateCw,
  Flag,
  Hourglass,
  ScrollText,
} from "lucide-react";
import { cn, resolveGallerySrc } from "@/lib/utils";
import type { ToolStep } from "@/stores/skillsStore";
import { useGalleryStore } from "@/stores/galleryStore";

interface Props {
  steps: ToolStep[];
  /** Whether steps are still accumulating (streaming in progress) */
  isStreaming?: boolean;
}

function prettifyName(name: string): string {
  // "builtin-web-search__web_search" → "web search"
  const bare = name.includes("__") ? name.split("__").pop()! : name;
  return bare.replace(/_/g, " ");
}

// ── Browser-agent tool metadata ──────────────────────────────────────────────

/** Map a browser_agent tool short-name to its icon and accent color. */
function browserToolMeta(
  shortName: string
):
  | {
      Icon: React.ComponentType<{ className?: string }>;
      tone: string;
      border: string;
      bg: string;
    }
  | null {
  switch (shortName) {
    case "navigate":
      return {
        Icon: Globe,
        tone: "text-sky-300",
        border: "border-sky-500/30",
        bg: "bg-sky-500/5",
      };
    case "click":
      return {
        Icon: MousePointerClick,
        tone: "text-indigo-300",
        border: "border-indigo-500/30",
        bg: "bg-indigo-500/5",
      };
    case "type":
      return {
        Icon: Keyboard,
        tone: "text-violet-300",
        border: "border-violet-500/30",
        bg: "bg-violet-500/5",
      };
    case "press_key":
      return {
        Icon: Keyboard,
        tone: "text-violet-300",
        border: "border-violet-500/30",
        bg: "bg-violet-500/5",
      };
    case "snapshot":
      return {
        Icon: Camera,
        tone: "text-cyan-300",
        border: "border-cyan-500/30",
        bg: "bg-cyan-500/5",
      };
    case "extract":
      return {
        Icon: FileSearch,
        tone: "text-amber-300",
        border: "border-amber-500/30",
        bg: "bg-amber-500/5",
      };
    case "go_back":
      return {
        Icon: ArrowLeftCircle,
        tone: "text-slate-300",
        border: "border-slate-500/30",
        bg: "bg-slate-500/5",
      };
    case "reload":
      return {
        Icon: RotateCw,
        tone: "text-slate-300",
        border: "border-slate-500/30",
        bg: "bg-slate-500/5",
      };
    case "scroll":
      return {
        Icon: ScrollText,
        tone: "text-slate-300",
        border: "border-slate-500/30",
        bg: "bg-slate-500/5",
      };
    case "wait":
      return {
        Icon: Hourglass,
        tone: "text-slate-300",
        border: "border-slate-500/30",
        bg: "bg-slate-500/5",
      };
    case "done":
      return {
        Icon: Flag,
        tone: "text-emerald-300",
        border: "border-emerald-500/30",
        bg: "bg-emerald-500/5",
      };
    default:
      return null;
  }
}

/** Extract the short name (after the last `__`) for matching. */
function shortToolName(name: string): string {
  return name.includes("__") ? name.split("__").pop()! : name;
}

/** Compact preview for the most common browser args, shown inline in the header. */
function browserArgPreview(
  shortName: string,
  args: Record<string, unknown>
): string | null {
  switch (shortName) {
    case "navigate":
      return typeof args.url === "string" ? String(args.url) : null;
    case "click":
      return args.index !== undefined ? `[${args.index}]` : null;
    case "type": {
      const idx = args.index !== undefined ? `[${args.index}] ` : "";
      const txt =
        typeof args.text === "string"
          ? args.text.length > 48
            ? args.text.slice(0, 48) + "…"
            : args.text
          : "";
      return `${idx}${txt}`.trim() || null;
    }
    case "press_key":
      return typeof args.key === "string" ? String(args.key) : null;
    case "extract":
      return typeof args.query === "string" ? String(args.query) : null;
    default:
      return null;
  }
}

// ── Code execution result renderer ───────────────────────────────────────────

interface CodeRunResult {
  stdout?: string;
  stderr?: string;
  exit_code?: number;
  execution_time_ms?: number;
}

function CodeExecutionResult({ raw }: { raw: string }) {
  let result: CodeRunResult = {};
  try {
    result = JSON.parse(raw);
  } catch {
    return (
      <pre className="text-[10px] font-mono whitespace-pre-wrap break-all text-foreground/80 bg-black/20 rounded p-2 max-h-48 overflow-y-auto">
        {raw}
      </pre>
    );
  }

  const { stdout, stderr, exit_code, execution_time_ms } = result;
  const success = exit_code === 0 || exit_code === undefined;

  return (
    <div className="space-y-2">
      {/* Status bar */}
      <div className="flex items-center gap-2 text-[10px]">
        <span
          className={cn(
            "flex items-center gap-1 font-mono px-1.5 py-0.5 rounded border",
            success
              ? "bg-emerald-500/10 border-emerald-500/30 text-emerald-400"
              : "bg-red-500/10 border-red-500/30 text-red-400"
          )}
        >
          exit: {exit_code ?? 0}
        </span>
        {execution_time_ms !== undefined && (
          <span className="flex items-center gap-1 text-muted-foreground">
            <Clock className="w-2.5 h-2.5" />
            {execution_time_ms < 1000
              ? `${execution_time_ms}ms`
              : `${(execution_time_ms / 1000).toFixed(1)}s`}
          </span>
        )}
      </div>

      {/* stdout */}
      {stdout && stdout.trim() && (
        <div>
          <div className="text-[9px] font-semibold uppercase tracking-wider text-emerald-400/70 mb-0.5 flex items-center gap-1">
            <Terminal className="w-2.5 h-2.5" />
            stdout
          </div>
          <pre className="text-[10px] font-mono whitespace-pre-wrap break-words text-emerald-300/90 bg-emerald-950/40 border border-emerald-500/20 rounded p-2 max-h-52 overflow-y-auto leading-relaxed">
            {stdout}
          </pre>
        </div>
      )}

      {/* stderr */}
      {stderr && stderr.trim() && (
        <div>
          <div className="text-[9px] font-semibold uppercase tracking-wider text-red-400/70 mb-0.5 flex items-center gap-1">
            <AlertCircle className="w-2.5 h-2.5" />
            stderr
          </div>
          <pre className="text-[10px] font-mono whitespace-pre-wrap break-words text-red-300/90 bg-red-950/40 border border-red-500/20 rounded p-2 max-h-52 overflow-y-auto leading-relaxed">
            {stderr}
          </pre>
        </div>
      )}

      {/* Empty output */}
      {(!stdout || !stdout.trim()) && (!stderr || !stderr.trim()) && (
        <p className="text-[10px] text-muted-foreground italic">
          (no output)
        </p>
      )}
    </div>
  );
}

// ── Image generation result renderer ─────────────────────────────────────────

interface ImageGenResultData {
  status?: string;
  image_url?: string;
  /** Gallery DB id — present when the image was persisted to the gallery. */
  gallery_id?: string;
  filename?: string;
  width?: number;
  height?: number;
  prompt?: string;
}

// A persisted gallery image is served at this local URL, but the Tauri WebView
// cannot load it over HTTP (the HTTP server only runs when the mobile API is
// enabled). We must always resolve such images through the gallery store.
const LOCAL_GALLERY_RE = /\/images\/([^/?#]+)/;

/** Resolve the best image src for a generated image.
 *
 *  When the image was persisted to the gallery (`gallery_id`, or an
 *  `.../images/<id>` URL) we resolve it through the gallery store as an
 *  asset:// path / base64 data URL — the raw localhost HTTP URL is never
 *  loadable in the desktop WebView. While the gallery entry is still being
 *  fetched we report `pending` so the caller can show a skeleton instead of a
 *  broken image. Non-gallery URLs (e.g. a live ComfyUI `/view` URL) are used
 *  as-is. */
function useImageSrc(result: ImageGenResultData): { src?: string; pending: boolean } {
  const galleryImages = useGalleryStore((s) => s.images);

  // Determine the gallery id, either explicit or embedded in the URL.
  const galleryId =
    result.gallery_id ||
    (result.image_url ? result.image_url.match(LOCAL_GALLERY_RE)?.[1] : undefined) ||
    null;

  // Hold the last successfully-resolved src so transient gallery refetches
  // (which momentarily replace the list) don't flash a skeleton.
  const stableSrcRef = useRef<string | undefined>(undefined);

  // Trigger a gallery fetch when we have an id but the entry isn't loaded yet.
  const entry = galleryId ? galleryImages.find((img) => img.id === galleryId) : null;
  useEffect(() => {
    if (!galleryId || entry) return;
    const s = useGalleryStore.getState();
    if (s.scope === "all") s.fetchAllImages();
    else if (s.activeConversationId) s.fetchImages(s.activeConversationId);
  }, [galleryId, entry]);

  if (entry) {
    const resolved = resolveGallerySrc(entry);
    if (resolved) {
      stableSrcRef.current = resolved;
      return { src: resolved, pending: false };
    }
  }
  if (stableSrcRef.current) {
    return { src: stableSrcRef.current, pending: false };
  }
  // A gallery image we couldn't resolve yet → pending (don't show the raw
  // localhost URL). A non-gallery URL → use it directly.
  if (galleryId) return { src: undefined, pending: true };
  return { src: result.image_url, pending: false };
}


// ── Always-visible image preview (uses gallery data URL for persistence) ─────

function AlwaysVisibleImagePreview({ raw }: { raw: string }) {
  let r: ImageGenResultData = {};
  let parsed = true;
  try {
    r = JSON.parse(raw);
  } catch {
    parsed = false;
  }

  // eslint-disable-next-line react-hooks/rules-of-hooks
  const { src, pending } = useImageSrc(parsed ? r : {});
  if (!parsed) return null;

  if (pending) {
    return (
      <div className="border-t border-purple-500/20 px-3 pb-3 pt-2">
        <div className="rounded-md border border-border bg-muted/30 animate-pulse w-full h-48" />
      </div>
    );
  }

  if (!src) return null;

  return (
    <div className="border-t border-purple-500/20 px-3 pb-3 pt-2 space-y-2">
      <img
        src={src}
        alt={r.prompt ?? "Generated image"}
        className="rounded-md border border-border max-w-full max-h-96 object-contain"
      />
      <div className="flex flex-wrap gap-3 text-[10px] text-muted-foreground">
        {r.width && r.height && (
          <span>{r.width} × {r.height} px</span>
        )}
        {r.filename && (
          <span className="font-mono">{r.filename}</span>
        )}
      </div>
      {r.prompt && (
        <p className="text-[10px] text-foreground/70 italic line-clamp-2">
          "{r.prompt}"
        </p>
      )}
    </div>
  );
}

// ── Video generation result renderer ─────────────────────────────────────────

interface VideoGenResultData {
  status?: string;
  video_url?: string;
  gallery_id?: string;
  filename?: string;
  width?: number;
  height?: number;
  frames?: number;
  seed?: number;
  prompt?: string;
}

/** Resolve the best video src.
 *  For videos the gallery stores the URL (not base64) so we just read it back.
 *  Falls back to the live ComfyUI URL from the tool result. */
function useVideoSrc(result: VideoGenResultData): string | undefined {
  const galleryImages = useGalleryStore((s) => s.images);
  if (result.gallery_id) {
    const entry = galleryImages.find((img) => img.id === result.gallery_id);
    if (entry && entry.mime_type.startsWith("video/")) {
      return entry.image_data; // stored as URL for videos
    }
  }
  return result.video_url;
}

function AlwaysVisibleVideoPreview({ raw }: { raw: string }) {
  let r: VideoGenResultData = {};
  try {
    r = JSON.parse(raw);
  } catch {
    return null;
  }

  // eslint-disable-next-line react-hooks/rules-of-hooks
  const src = useVideoSrc(r);
  if (!src) return null;

  const isGif = r.filename?.toLowerCase().endsWith(".gif");

  return (
    <div className="border-t border-teal-500/20 px-3 pb-3 pt-2 space-y-2">
      {isGif ? (
        <img
          src={src}
          alt={r.prompt ?? "Generated video"}
          className="rounded-md border border-border max-w-full max-h-96 object-contain"
        />
      ) : (
        <video
          src={src}
          controls
          loop
          className="rounded-md border border-border max-w-full max-h-96 w-full"
        />
      )}
      <div className="flex flex-wrap gap-3 text-[10px] text-muted-foreground">
        {r.width && r.height && (
          <span>{r.width} × {r.height} px</span>
        )}
        {r.frames && (
          <span>{r.frames} frames</span>
        )}
        {r.filename && (
          <span className="font-mono">{r.filename}</span>
        )}
      </div>
      {r.prompt && (
        <p className="text-[10px] text-foreground/70 italic line-clamp-2">
          "{r.prompt}"
        </p>
      )}
    </div>
  );
}

// ── Generic tool step card ────────────────────────────────────────────────────

const ToolStepCard = memo(function ToolStepCard({
  step,
  isLast,
  isStreaming,
}: {
  step: ToolStep;
  isLast: boolean;
  isStreaming?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const pending = isLast && isStreaming && step.result === undefined;
  const isError = step.result?.includes('"error"') ?? false;

  const isCodeExec = step.function_name
    ?.toLowerCase()
    .includes("execute_code");

  const isImageGen = step.function_name
    ?.toLowerCase()
    .includes("generate_image");

  const isVideoGen = step.function_name
    ?.toLowerCase()
    .includes("generate_video");

  const isBrowserAgent = step.function_name
    ?.toLowerCase()
    .startsWith("browser_agent");
  const browserMeta = isBrowserAgent
    ? browserToolMeta(shortToolName(step.function_name))
    : null;
  const browserPreview = isBrowserAgent
    ? browserArgPreview(shortToolName(step.function_name), step.arguments || {})
    : null;

  // Auto-open code execution, image, and video generation steps so output is visible immediately.
  // Done in useEffect (not during render) to comply with React rules and avoid extra render cycles.
  const [autoOpened, setAutoOpened] = useState(false);
  useEffect(() => {
    if ((isCodeExec || isImageGen || isVideoGen) && !pending && step.result !== undefined && !autoOpened) {
      setAutoOpened(true);
      setOpen(true);
    }
  }, [step.result, pending, isCodeExec, isImageGen, isVideoGen, autoOpened]);

  return (
    <div
      className={cn(
        "rounded-lg border text-xs overflow-hidden transition-colors",
        pending
          ? "border-amber-500/30 bg-amber-500/5"
          : isError
          ? "border-red-500/30 bg-red-500/5"
          : isVideoGen
          ? "border-teal-500/30 bg-teal-500/5"
          : isImageGen
          ? "border-purple-500/30 bg-purple-500/5"
          : isCodeExec
          ? "border-blue-500/30 bg-blue-500/5"
          : browserMeta
          ? cn(browserMeta.border, browserMeta.bg)
          : "border-emerald-500/30 bg-emerald-500/5"
      )}
    >
      {/* Header row */}
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-white/5 transition-colors"
      >
        {pending ? (
          <Loader2 className="w-3 h-3 text-amber-400 animate-spin shrink-0" />
        ) : isError ? (
          <AlertCircle className="w-3 h-3 text-red-400 shrink-0" />
        ) : isVideoGen ? (
          <Video className="w-3 h-3 text-teal-400 shrink-0" />
        ) : isImageGen ? (
          <ImageIcon className="w-3 h-3 text-purple-400 shrink-0" />
        ) : isCodeExec ? (
          <Terminal className="w-3 h-3 text-blue-400 shrink-0" />
        ) : browserMeta ? (
          <browserMeta.Icon className={cn("w-3 h-3 shrink-0", browserMeta.tone)} />
        ) : (
          <CheckCircle2 className="w-3 h-3 text-emerald-400 shrink-0" />
        )}
        {!browserMeta && (
          <Wrench className="w-3 h-3 text-muted-foreground shrink-0" />
        )}
        <span
          className={cn(
            "font-medium capitalize",
          pending
            ? "text-amber-300"
            : isError
            ? "text-red-300"
            : isVideoGen
            ? "text-teal-300"
            : isImageGen
            ? "text-purple-300"
            : isCodeExec
            ? "text-blue-300"
            : browserMeta
            ? browserMeta.tone
            : "text-emerald-300"
          )}
        >
          {prettifyName(step.function_name)}
        </span>
        {browserPreview && (
          <span
            className="ml-1 min-w-0 truncate text-[10px] text-muted-foreground/80 font-mono"
            title={browserPreview}
          >
            {browserPreview}
          </span>
        )}
        <span className="ml-auto text-muted-foreground/50 text-[10px]">
          turn {step.turn + 1}
        </span>
        {open ? (
          <ChevronDown className="w-3 h-3 text-muted-foreground shrink-0" />
        ) : (
          <ChevronRight className="w-3 h-3 text-muted-foreground shrink-0" />
        )}
      </button>

      {/* Collapsible details: image preview + arguments + results */}
      {open && (
        <div className="border-t border-white/10 px-3 pb-3 pt-2 space-y-2">
          {/* Image shown at the top of the expanded card */}
          {isImageGen && !pending && step.result !== undefined && (
            <AlwaysVisibleImagePreview raw={step.result} />
          )}
          {/* Video shown at the top of the expanded card */}
          {isVideoGen && !pending && step.result !== undefined && (
            <AlwaysVisibleVideoPreview raw={step.result} />
          )}
          <div>
            <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mb-1">
              Arguments
            </div>
            {isCodeExec && step.arguments?.code ? (
              <pre className="text-[10px] font-mono whitespace-pre-wrap break-all text-blue-200/80 bg-blue-950/30 border border-blue-500/20 rounded p-2 max-h-48 overflow-y-auto">
                {String(step.arguments.code ?? "")}
              </pre>
            ) : (
              <pre className="text-[10px] font-mono whitespace-pre-wrap break-all text-foreground/80 bg-black/20 rounded p-2 max-h-32 overflow-y-auto">
                {JSON.stringify(step.arguments, null, 2)}
              </pre>
            )}
          </div>
          {/* For image/video gen the preview above already shows everything.
              For other tools show the result / output section. */}
          {step.result !== undefined && !isImageGen && !isVideoGen && (
            <div>
              <div
                className={cn(
                  "text-[10px] font-semibold uppercase tracking-wider mb-1",
                  isError
                    ? "text-red-400"
                    : isCodeExec
                    ? "text-blue-400"
                    : "text-emerald-400"
                )}
              >
                {isError ? "Error" : isCodeExec ? "Output" : "Result"}
              </div>
              {isCodeExec ? (
                <CodeExecutionResult raw={step.result} />
              ) : (
                <pre className="text-[10px] font-mono whitespace-pre-wrap break-all text-foreground/80 bg-black/20 rounded p-2 max-h-48 overflow-y-auto">
                  {(() => {
                    try {
                      return JSON.stringify(JSON.parse(step.result!), null, 2);
                    } catch {
                      return step.result;
                    }
                  })()}
                </pre>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
});

export const ToolCallMessage = memo(function ToolCallMessage({ steps, isStreaming }: Props) {
  if (steps.length === 0) return null;

  return (
    <div className="space-y-1.5 w-full">
      {steps.map((step, i) => (
        <ToolStepCard
          key={step.tool_call_id}
          step={step}
          isLast={i === steps.length - 1}
          isStreaming={isStreaming}
        />
      ))}
    </div>
  );
});
