import { useState, useRef, KeyboardEvent, useMemo, useEffect } from "react";
import {
  Send,
  BookOpen,
  Wrench,
  Paperclip,
  X,
  FileText,
  FileCode,
  FileJson,
  File,
  Cpu,
  Loader2,
  Globe,
  ImageIcon,
} from "lucide-react";
import { readFile } from "@tauri-apps/plugin-fs";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useChatStore } from "@/stores/chatStore";
import { useSkillsStore } from "@/stores/skillsStore";
import { useServerStore } from "@/stores/serverStore";
import { Button } from "@/components/ui/button";
import { cn, bytesToBase64, imageMime } from "@/lib/utils";
import type { RagCollection } from "@/lib/tauri";

interface Props {
  collections: RagCollection[];
  disabled?: boolean;
}

const IMAGE_EXTS = new Set(["jpg", "jpeg", "png", "gif", "webp", "bmp"]);

function isImagePath(path: string): boolean {
  const ext = path.replace(/\\/g, "/").split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTS.has(ext);
}

function fileExtIcon(filename: string) {
  const ext = filename.split(".").pop()?.toLowerCase() ?? "";
  if (["csv", "tsv"].includes(ext)) return <FileText className="w-3 h-3" />;
  if (["json", "jsonl"].includes(ext)) return <FileJson className="w-3 h-3" />;
  if (["js", "ts", "py", "rs", "go", "java", "cpp", "c", "cs"].includes(ext))
    return <FileCode className="w-3 h-3" />;
  return <File className="w-3 h-3" />;
}

function basename(path: string): string {
  return path.replace(/\\/g, "/").split("/").pop() ?? path;
}

/** Loads image files as base64 data-URLs for preview thumbnails. */
function useImagePreviews(paths: string[]): Map<string, string> {
  const [previews, setPreviews] = useState<Map<string, string>>(new Map());

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      const next = new Map<string, string>();
      for (const p of paths) {
        if (!isImagePath(p)) continue;
        try {
          const bytes = await readFile(p);
          const ext = p.split(".").pop() ?? "jpeg";
          const mime = imageMime(ext);
          const b64 = bytesToBase64(new Uint8Array(bytes));
          next.set(p, `data:${mime};base64,${b64}`);
        } catch {
          // If the file can't be read, skip the thumbnail
        }
      }
      if (!cancelled) setPreviews(next);
    };
    void load();
    return () => { cancelled = true; };
  }, [paths.join("|")]);  // eslint-disable-line react-hooks/exhaustive-deps

  return previews;
}

export function InputBar({ collections, disabled }: Props) {
  const [content, setContent] = useState("");
  const [useRag, setUseRag] = useState(false);
  const [selectedCollection, setSelectedCollection] = useState<string | null>(null);
  const [attachedPaths, setAttachedPaths] = useState<string[]>([]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const imagePreviews = useImagePreviews(attachedPaths);
  const sendMessage = useChatStore((s) => s.sendMessage);
  const { tools, skillsEnabled, toggleSkills, clearToolSteps } = useSkillsStore();

  // Server / model state
  const { status, isStarting, engineMode, lastModel, startServer } = useServerStore();

  // True when we're in local mode and the server isn't up yet
  const needsLoad = engineMode === "local" && !status.running && !isStarting;
  // Can actually start a model (has a previously-used model path)
  const canLoad = needsLoad && !!lastModel;

  const handleSend = async (overrideText?: string) => {
    const text = (overrideText ?? content).trim();
    if (!text || disabled) return;
    clearToolSteps();
    if (!overrideText) {
      // Only clear the textarea when sending normally (not from handleLoadAndSend)
      setContent("");
      if (textareaRef.current) {
        textareaRef.current.style.height = "auto";
      }
    }
    const pathsToSend = [...attachedPaths];
    setAttachedPaths([]);
    await sendMessage(
      text,
      useRag,
      selectedCollection || undefined,
      skillsEnabled,
      pathsToSend.length > 0 ? pathsToSend : undefined,
    );
  };

  const handleLoadAndSend = async () => {
    const text = content.trim();
    if (!text || !lastModel) return;

    // Clear textarea immediately so the user sees activity
    setContent("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }

    // Start the server; startServer sets isStarting = true while loading
    await startServer(lastModel);

    // Only send if server started without errors
    const { error } = useServerStore.getState();
    if (!error) {
      await handleSend(text);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (canLoad) {
        handleLoadAndSend();
      } else {
        handleSend();
      }
    }
  };

  // Detect URLs in the current message for the visual preview chips.
  const detectedUrls = useMemo(() => {
    const re = /https?:\/\/[^\s>"')\]]+/g;
    const matches = content.match(re) ?? [];
    // Deduplicate and trim trailing punctuation
    return [...new Set(matches.map((u) => u.replace(/[.,;:!?]+$/, "")))];
  }, [content]);

  const handleInput = () => {
    const ta = textareaRef.current;
    if (!ta) return;
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, 200) + "px";
  };

  const handleAttach = async () => {
    const selected = await openDialog({
      multiple: true,
      filters: [
        {
          name: "All supported",
          extensions: [
            // Images (VLM)
            "jpg", "jpeg", "png", "gif", "webp", "bmp",
            // Documents & data
            "csv", "tsv", "json", "jsonl", "txt", "md", "pdf",
            // Code
            "js", "ts", "py", "rs", "go", "java", "cpp", "c", "cs",
            "html", "xml", "yaml", "yml",
          ],
        },
        {
          name: "Images",
          extensions: ["jpg", "jpeg", "png", "gif", "webp", "bmp"],
        },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    setAttachedPaths((prev) => {
      const existing = new Set(prev);
      return [...prev, ...paths.filter((p) => !existing.has(p))];
    });
  };

  const removeAttachment = (path: string) => {
    setAttachedPaths((prev) => prev.filter((p) => p !== path));
  };

  // ── Derived send-button props ──────────────────────────────────────────────
  type BtnVariant = "default" | "outline" | "secondary" | "ghost" | "link" | "destructive";

  const sendButtonProps: {
    onClick: () => void;
    disabled: boolean;
    title?: string;
    variant?: BtnVariant;
    wide?: boolean;
  } = (() => {
    if (isStarting) {
      return {
        onClick: () => {},
        disabled: true,
        title: "Starting model…",
        wide: true,
      };
    }
    if (needsLoad && lastModel) {
      return {
        onClick: () => handleLoadAndSend(),
        disabled: !content.trim() || !!disabled,
        title: `Load ${basename(lastModel)} and send`,
        wide: true,
      };
    }
    if (needsLoad && !lastModel) {
      return {
        onClick: () => {},
        disabled: true,
        title: "No model selected — go to Settings to load a model",
        wide: true,
      };
    }
    // Normal send — wrap in arrow function so no SyntheticEvent is passed as overrideText
    return {
      onClick: () => handleSend(),
      disabled: !content.trim() || !!disabled,
    };
  })();

  const placeholderText = (() => {
    if (disabled) return "Generating…";
    if (isStarting) return "Starting model…";
    if (needsLoad) return "Type your message — click Load Model to start the AI";
    return "Message XandSuite… (Enter to send, Shift+Enter for newline)";
  })();

  return (
    <div className="px-4 pb-4 pt-2 border-t border-border shrink-0">
      {/* Toggle row (RAG + Skills) */}
      {(collections.length > 0 || tools.length > 0) && (
        <div className="flex items-center gap-2 mb-2">
          {/* RAG toggle */}
          {collections.length > 0 && (
            <>
              <button
                className={cn(
                  "flex items-center gap-1 px-2 py-1 rounded-md text-xs transition-colors",
                  useRag
                    ? "bg-primary/20 text-primary border border-primary/30"
                    : "bg-secondary text-muted-foreground hover:text-foreground"
                )}
                onClick={() => setUseRag((v) => !v)}
              >
                <BookOpen className="w-3 h-3" />
                RAG
              </button>

              {useRag && (
                <select
                  className="text-xs bg-secondary border border-border rounded-md px-2 py-1 text-foreground"
                  value={selectedCollection || ""}
                  onChange={(e) => setSelectedCollection(e.target.value || null)}
                >
                  <option value="">All collections</option>
                  {collections.map((c) => (
                    <option key={c.id} value={c.id}>{c.name}</option>
                  ))}
                </select>
              )}
            </>
          )}

          {/* Skills / Tools toggle */}
          {tools.length > 0 && (
            <button
              className={cn(
                "flex items-center gap-1 px-2 py-1 rounded-md text-xs transition-colors",
                skillsEnabled
                  ? "bg-violet-500/20 text-violet-300 border border-violet-500/30"
                  : "bg-secondary text-muted-foreground hover:text-foreground"
              )}
              onClick={toggleSkills}
              title={skillsEnabled ? `${tools.length} tool${tools.length !== 1 ? "s" : ""} enabled` : "Enable skills"}
            >
              <Wrench className="w-3 h-3" />
              Tools {skillsEnabled && <span className="text-[10px] opacity-70">({tools.length})</span>}
            </button>
          )}
        </div>
      )}

      {/* URL fetch chips — shown when the message contains HTTP/S links */}
      {detectedUrls.length > 0 && (
        <div className="flex flex-wrap gap-1.5 mb-2">
          {detectedUrls.map((url) => {
            const display = url.replace(/^https?:\/\//, "").replace(/\/$/, "");
            return (
              <div
                key={url}
                className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-blue-500/10 border border-blue-500/30 text-xs text-blue-400"
                title={`This URL will be fetched and its content injected into the AI context: ${url}`}
              >
                <Globe className="w-3 h-3 shrink-0" />
                <span className="max-w-[220px] truncate">{display}</span>
                <span className="text-blue-500/60 text-[10px] ml-0.5">will fetch</span>
              </div>
            );
          })}
        </div>
      )}

      {/* Attachment chips — images shown as thumbnails, other files as icon chips */}
      {attachedPaths.length > 0 && (
        <div className="flex flex-wrap gap-1.5 mb-2">
          {attachedPaths.map((path) => {
            const name = basename(path);
            const preview = imagePreviews.get(path);
            const isImg = isImagePath(path);

            if (isImg) {
              return (
                <div key={path} className="relative group">
                  <div className="w-14 h-14 rounded-lg border border-border overflow-hidden bg-secondary">
                    {preview ? (
                      <img
                        src={preview}
                        alt={name}
                        className="w-full h-full object-cover"
                        title={name}
                      />
                    ) : (
                      <div className="w-full h-full flex items-center justify-center text-muted-foreground">
                        <ImageIcon className="w-5 h-5" />
                      </div>
                    )}
                  </div>
                  <button
                    onClick={() => removeAttachment(path)}
                    className="absolute -top-1.5 -right-1.5 w-4 h-4 rounded-full bg-background border border-border
                               flex items-center justify-center text-muted-foreground hover:text-foreground
                               opacity-0 group-hover:opacity-100 transition-opacity"
                    aria-label={`Remove ${name}`}
                  >
                    <X className="w-2.5 h-2.5" />
                  </button>
                </div>
              );
            }

            return (
              <div
                key={path}
                className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-secondary border border-border text-xs text-foreground"
              >
                <span className="text-muted-foreground">{fileExtIcon(name)}</span>
                <span className="max-w-[160px] truncate">{name}</span>
                <button
                  onClick={() => removeAttachment(path)}
                  className="ml-0.5 text-muted-foreground hover:text-foreground transition-colors"
                  aria-label="Remove attachment"
                >
                  <X className="w-3 h-3" />
                </button>
              </div>
            );
          })}
        </div>
      )}

      {/* Input area */}
      <div className={cn(
        "flex items-end gap-2 rounded-xl border border-border bg-card px-3 py-2 transition-colors",
        !disabled && "focus-within:border-primary/50"
      )}>
        {/* Attach button */}
        <button
          className="shrink-0 text-muted-foreground hover:text-foreground transition-colors pb-1"
          onClick={handleAttach}
          disabled={disabled || isStarting}
          title="Attach file"
        >
          <Paperclip className="w-4 h-4" />
        </button>

        <textarea
          ref={textareaRef}
          className="flex-1 bg-transparent text-sm resize-none outline-none placeholder:text-muted-foreground min-h-[36px] max-h-[200px] leading-relaxed"
          placeholder={placeholderText}
          value={content}
          onChange={(e) => setContent(e.target.value)}
          onKeyDown={handleKeyDown}
          onInput={handleInput}
          disabled={disabled || isStarting}
          rows={1}
        />

        {/* Send / Load Model button */}
        {sendButtonProps.wide ? (
          <Button
            size="sm"
            className={cn(
              "shrink-0 h-8 gap-1.5 px-3 text-xs font-medium transition-all",
              canLoad && !isStarting && "bg-primary hover:bg-primary/90"
            )}
            onClick={sendButtonProps.onClick}
            disabled={sendButtonProps.disabled}
            title={sendButtonProps.title}
            variant={sendButtonProps.variant}
          >
            {isStarting ? (
              <>
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
                Starting…
              </>
            ) : (
              <>
                <Cpu className="w-3.5 h-3.5" />
                Load Model
              </>
            )}
          </Button>
        ) : (
          <Button
            size="icon"
            className="h-8 w-8 shrink-0"
            onClick={sendButtonProps.onClick}
            disabled={sendButtonProps.disabled}
            title={sendButtonProps.title}
          >
            <Send className="w-3.5 h-3.5" />
          </Button>
        )}
      </div>
    </div>
  );
}
