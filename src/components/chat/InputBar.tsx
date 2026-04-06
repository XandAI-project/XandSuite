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
  AlertCircle,
  Mic,
  MicOff,
} from "lucide-react";
import { useNavigate } from "react-router-dom";
import { readFile } from "@tauri-apps/plugin-fs";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useChatStore } from "@/stores/chatStore";
import { useSkillsStore } from "@/stores/skillsStore";
import { useServerStore } from "@/stores/serverStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { Button } from "@/components/ui/button";
import { cn, bytesToBase64, imageMime } from "@/lib/utils";
import { useVoiceInput } from "@/hooks/useVoiceInput";
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
  const [showNoModelModal, setShowNoModelModal] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const imagePreviews = useImagePreviews(attachedPaths);
  const sendMessage = useChatStore((s) => s.sendMessage);
  const { tools, skillsEnabled, toggleSkills, clearToolSteps } = useSkillsStore();
  const navigate = useNavigate();

  // Server / model state
  const { status, isStarting, engineMode, lastModel, startServer } = useServerStore();

  // Voice input (Whisper)
  const { settings } = useSettingsStore();
  const whisperEnabled = settings?.whisper_enabled ?? false;
  const [micError, setMicError] = useState<string | null>(null);

  // True when we're in local mode and the server isn't up yet
  const needsLoad = engineMode === "local" && !status.running && !isStarting;

  const handleSend = async (overrideText?: string) => {
    const text = (overrideText ?? content).trim();
    if (!text || disabled) return;

    // Gate: model must be running (or we're in remote mode)
    if (needsLoad) {
      setShowNoModelModal(true);
      return;
    }

    clearToolSteps();
    setContent("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
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

  // Ref so the onTranscript closure can call stopVoice even though stopVoice
  // is only available after useVoiceInput returns.
  const stopVoiceRef = useRef<() => void>(() => {});

  const { active: recording, transcribing, start: startVoice, stop: stopVoice } = useVoiceInput({
    onTranscript: (text) => {
      handleSend(text);
      // Stop listening after sending — 3 s of silence means the turn is done
      stopVoiceRef.current();
    },
    onTranscribing: () => {},
    onError: (msg) => setMicError(msg),
    language: settings?.whisper_language ?? "auto",
    silenceMs: 3000,
  });

  // Keep ref current so the stale onTranscript closure always stops the right session
  stopVoiceRef.current = stopVoice;

  const handleLoadAndSend = async () => {
    const text = content.trim();
    if (!text || !lastModel) return;

    setShowNoModelModal(false);
    setContent("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }

    await startServer(lastModel);

    const { error } = useServerStore.getState();
    if (!error) {
      await handleSend(text);
    }
  };

  const handleMic = async () => {
    setMicError(null);
    if (recording) {
      stopVoice();
    } else {
      try {
        await startVoice();
      } catch (e) {
        setMicError(e instanceof Error ? e.message : "Microphone access denied");
      }
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (needsLoad) {
        setShowNoModelModal(true);
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
  const sendButtonIsStarting = isStarting;
  const sendButtonDisabled = !content.trim() || !!disabled || isStarting;

  const placeholderText = (() => {
    if (disabled) return "Generating…";
    if (isStarting) return "Starting model…";
    if (needsLoad) return "Type a message — press Enter or Send to load a model first";
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
        disabled ? "opacity-60" : "focus-within:border-primary/50"
      )}>
        {/* Action buttons group */}
        <div className="flex items-center gap-1 pb-1 shrink-0">
          {/* Attach button */}
          <button
            className={cn(
              "flex items-center justify-center w-7 h-7 rounded-lg transition-all",
              disabled || isStarting
                ? "text-muted-foreground/40 cursor-not-allowed"
                : "text-muted-foreground hover:text-foreground hover:bg-secondary"
            )}
            onClick={handleAttach}
            disabled={disabled || isStarting}
            title="Attach file"
          >
            <Paperclip className="w-4 h-4" />
          </button>

          {/* Microphone button — only shown when Whisper is enabled in settings */}
          {whisperEnabled && (
            <button
              className={cn(
                "relative flex items-center justify-center w-7 h-7 rounded-lg transition-all",
                disabled || isStarting
                  ? "text-muted-foreground/40 cursor-not-allowed"
                  : transcribing
                  ? "text-primary bg-primary/10 cursor-wait"
                  : recording
                  ? "text-red-400 bg-red-500/10 hover:bg-red-500/20"
                  : "text-muted-foreground hover:text-foreground hover:bg-secondary"
              )}
              onClick={handleMic}
              disabled={disabled || isStarting}
              title={
                disabled
                  ? "Cannot use voice while generating"
                  : transcribing
                  ? "Transcribing…"
                  : recording
                  ? "Voice active — speak freely. Click to stop."
                  : "Start voice input"
              }
            >
              {transcribing ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : recording ? (
                <>
                  <MicOff className="w-4 h-4" />
                  {/* Live recording indicator dot */}
                  <span className="absolute top-0.5 right-0.5 w-1.5 h-1.5 rounded-full bg-red-500 animate-pulse" />
                </>
              ) : (
                <Mic className="w-4 h-4" />
              )}
            </button>
          )}
        </div>

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

        {/* Send button — always visible; shows spinner while model is starting */}
        <Button
          size="icon"
          className="h-8 w-8 shrink-0"
          onClick={() => sendButtonIsStarting ? undefined : handleSend()}
          disabled={sendButtonDisabled}
          title={
            isStarting ? "Starting model…" :
            needsLoad ? "Load a model first" :
            "Send message"
          }
        >
          {sendButtonIsStarting
            ? <Loader2 className="w-3.5 h-3.5 animate-spin" />
            : <Send className="w-3.5 h-3.5" />
          }
        </Button>
      </div>
      {/* Mic error toast */}
      {micError && (
        <div className="mt-1 text-xs text-destructive flex items-center gap-1 px-1">
          <AlertCircle className="w-3 h-3 shrink-0" />
          <span>{micError}</span>
          <button className="ml-auto opacity-60 hover:opacity-100" onClick={() => setMicError(null)}>
            <X className="w-3 h-3" />
          </button>
        </div>
      )}

      {/* No-model modal */}
      {showNoModelModal && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
          onClick={() => setShowNoModelModal(false)}
        >
          <div
            className="bg-card border border-border rounded-2xl shadow-2xl p-6 w-full max-w-sm mx-4 flex flex-col gap-4"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Header */}
            <div className="flex items-start gap-3">
              <div className="p-2 rounded-xl bg-yellow-500/10 border border-yellow-500/20 shrink-0">
                <AlertCircle className="w-5 h-5 text-yellow-400" />
              </div>
              <div>
                <h3 className="font-semibold text-base">No model loaded</h3>
                <p className="text-sm text-muted-foreground mt-0.5">
                  {lastModel
                    ? `Load "${basename(lastModel)}" to start chatting, or browse models to choose a different one.`
                    : "You need to select and load a model before you can send messages."}
                </p>
              </div>
            </div>

            {/* Actions */}
            <div className="flex flex-col gap-2">
              {lastModel && (
                <Button
                  className="w-full gap-2"
                  onClick={handleLoadAndSend}
                  disabled={isStarting}
                >
                  {isStarting ? (
                    <><Loader2 className="w-4 h-4 animate-spin" /> Starting…</>
                  ) : (
                    <><Cpu className="w-4 h-4" /> Load &amp; Send</>
                  )}
                </Button>
              )}
              <Button
                variant="outline"
                className="w-full"
                onClick={() => { setShowNoModelModal(false); navigate("/models"); }}
              >
                Browse Models
              </Button>
              <button
                className="text-xs text-muted-foreground hover:text-foreground transition-colors text-center py-1"
                onClick={() => setShowNoModelModal(false)}
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
