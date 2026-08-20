import { useEffect, useMemo, useRef, useState, useCallback, memo } from "react";
import { Plus, Trash2, MessageSquare, Settings2, Check, X, Images, Zap, Sparkles, Pencil } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { useChatStore } from "@/stores/chatStore";
import { useRagStore } from "@/stores/ragStore";
import { useSkillsStore } from "@/stores/skillsStore";
import { useArtifactStore } from "@/stores/artifactStore";
import { useGalleryStore } from "@/stores/galleryStore";
import { usePersonaStore } from "@/stores/personaStore";
import { useTemplateStore } from "@/stores/templateStore";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { MessageBubble } from "./MessageBubble";
import { ToolCallMessage } from "./ToolCallMessage";
import { InputBar } from "./InputBar";
import { ArtifactPanel } from "./ArtifactPanel";
import { GalleryPanel } from "./GalleryPanel";
import { cn, formatDate } from "@/lib/utils";
import type { Message } from "@/lib/tauri";
import type { ToolStep } from "@/stores/skillsStore";

// ── Streaming bubble wrapper ───────────────────────────────────────────────────
//
// Isolated component that subscribes to streamingContent/streamingThinking/
// isThinking. This means only this one component re-renders on every token —
// the rest of the message list (and ChatView itself) remains stable.

interface StreamingBubbleWrapperProps {
  message: Message;
  activeToolSteps: ToolStep[];
  isStreaming: boolean;
  personaAvatar?: string;
  personaName?: string;
  messagesEndRef: React.RefObject<HTMLDivElement | null>;
}

const StreamingBubbleWrapper = memo(function StreamingBubbleWrapper({
  message,
  activeToolSteps,
  isStreaming,
  personaAvatar,
  personaName,
  messagesEndRef,
}: StreamingBubbleWrapperProps) {
  const streamingContent = useChatStore((s) => s.streamingContent);
  const streamingThinking = useChatStore((s) => s.streamingThinking);
  const isThinking = useChatStore((s) => s.isThinking);

  // Debounced auto-scroll during streaming so the view follows new tokens.
  const scrollTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (!isStreaming) return;
    if (scrollTimer.current) clearTimeout(scrollTimer.current);
    scrollTimer.current = setTimeout(() => {
      messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }, 80);
    return () => {
      if (scrollTimer.current) clearTimeout(scrollTimer.current);
    };
  }, [streamingContent, isStreaming, messagesEndRef]);

  return (
    <div className="space-y-2">
      {activeToolSteps.length > 0 && (
        <ToolCallMessage steps={activeToolSteps} isStreaming={isStreaming} />
      )}
      <MessageBubble
        message={message}
        liveContent={streamingContent}
        liveThinking={streamingThinking}
        isThinking={isThinking}
        personaAvatar={personaAvatar}
        personaName={personaName}
      />
    </div>
  );
});

export function ChatView() {
  // Narrow per-field selectors so each subscription only re-renders ChatView
  // when that specific field changes. Actions are stable Zustand references
  // that never change — selecting them individually avoids whole-store churn.

  // chatStore — reactive fields
  const conversations    = useChatStore((s) => s.conversations);
  const activeConversation = useChatStore((s) => s.activeConversation);
  const isStreaming      = useChatStore((s) => s.isStreaming);
  // chatStore — stable actions (never cause re-renders)
  const fetchConversations = useChatStore((s) => s.fetchConversations);
  const openConversation   = useChatStore((s) => s.openConversation);
  const createConversation = useChatStore((s) => s.createConversation);
  const updateConversation = useChatStore((s) => s.updateConversation);
  const deleteConversation = useChatStore((s) => s.deleteConversation);
  const renameConversation = useChatStore((s) => s.renameConversation);
  const retryLastMessage   = useChatStore((s) => s.retryLastMessage);
  const editAndResend      = useChatStore((s) => s.editAndResend);

  // ragStore
  const collections    = useRagStore((s) => s.collections);
  const fetchCollections = useRagStore((s) => s.fetchCollections);

  // skillsStore
  const activeToolSteps      = useSkillsStore((s) => s.activeToolSteps);
  const completedToolSteps   = useSkillsStore((s) => s.completedToolSteps);
  const snapshotCompletedSteps = useSkillsStore((s) => s.snapshotCompletedSteps);
  const clearToolSteps       = useSkillsStore((s) => s.clearToolSteps);
  const fetchTools           = useSkillsStore((s) => s.fetchTools);

  // artifactStore
  const artifacts      = useArtifactStore((s) => s.artifacts);
  const panelOpen      = useArtifactStore((s) => s.panelOpen);
  const fetchArtifacts = useArtifactStore((s) => s.fetchArtifacts);
  const clearArtifacts = useArtifactStore((s) => s.clearArtifacts);
  const flashAiEdited  = useArtifactStore((s) => s.flashAiEdited);

  // galleryStore
  const galleryOpen          = useGalleryStore((s) => s.galleryOpen);
  const toggleGallery        = useGalleryStore((s) => s.toggleGallery);
  const fetchImages          = useGalleryStore((s) => s.fetchImages);
  const fetchAllImages       = useGalleryStore((s) => s.fetchAllImages);
  const galleryScope         = useGalleryStore((s) => s.scope);
  const setGalleryConversation = useGalleryStore((s) => s.setActiveConversation);

  // personaStore / templateStore
  const personas      = usePersonaStore((s) => s.personas);
  const fetchPersonas = usePersonaStore((s) => s.fetchPersonas);
  const templates     = useTemplateStore((s) => s.templates);
  const fetchTemplates = useTemplateStore((s) => s.fetchTemplates);

  // Resolve the active persona for the current conversation (for avatar display)
  const activePersona = useMemo(() => {
    const pid = activeConversation?.persona_id;
    if (!pid) return null;
    return personas.find((p) => p.id === pid) ?? null;
  }, [activeConversation?.persona_id, personas]);

  const messagesEndRef = useRef<HTMLDivElement>(null);

  // System prompt editor state
  const [showSystemPrompt, setShowSystemPrompt] = useState(false);
  const [systemPromptDraft, setSystemPromptDraft] = useState("");

  // Delete confirmation dialog
  const [deleteTargetId, setDeleteTargetId] = useState<string | null>(null);

  // Inline rename state
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);

  // Focus rename input when it appears
  useEffect(() => {
    if (renamingId) {
      setTimeout(() => renameInputRef.current?.focus(), 30);
    }
  }, [renamingId]);

  // Track previous streaming state to detect the transition true → false
  const prevStreamingRef = useRef(false);

  useEffect(() => {
    fetchConversations();
    fetchCollections();
    fetchTools();
    fetchPersonas();
    fetchTemplates();
  }, []);  // eslint-disable-line react-hooks/exhaustive-deps

  // Pick up to 4 starter template suggestions for the welcome screen
  const suggestionTemplates = useMemo(() => {
    const preferred = ["builtin-summarise", "builtin-explain", "builtin-translate", "builtin-report"];
    const ordered = [
      ...preferred.map((id) => templates.find((t) => t.id === id)).filter(Boolean),
      ...templates.filter((t) => !preferred.includes(t.id)),
    ] as typeof templates;
    return ordered.slice(0, 4);
  }, [templates]);

  // Scroll when the number of finalized messages changes (conversation open,
  // message sent, stream done). Per-token scroll is handled inside
  // StreamingBubbleWrapper so this never fires during the token stream.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [activeConversation?.messages.length]);

  // Reload artifacts and gallery whenever the active conversation changes.
  // Also clear any stale tool-step state from the previous conversation.
  useEffect(() => {
    if (activeConversation?.id) {
      clearArtifacts();
      fetchArtifacts(activeConversation.id);
      clearToolSteps();
      setGalleryConversation(activeConversation.id);
      if (galleryScope === "conversation") {
        fetchImages(activeConversation.id);
      } else {
        fetchAllImages();
      }
    }
  }, [activeConversation?.id]);

  // Sync system prompt draft when conversation changes
  useEffect(() => {
    setSystemPromptDraft(activeConversation?.system_prompt ?? "");
    setShowSystemPrompt(false);
  }, [activeConversation?.id]);

  // When streaming ends:
  // 1. Re-fetch artifacts (backend saves them before emitting done:true)
  // 2. Snapshot any active tool steps into completedToolSteps so they remain
  //    visible in the last assistant message after streaming finishes.
  useEffect(() => {
    const wasStreaming = prevStreamingRef.current;
    prevStreamingRef.current = isStreaming;
    if (wasStreaming && !isStreaming) {
      if (activeConversation?.id) {
        fetchArtifacts(activeConversation.id);
      }
      if (activeToolSteps.length > 0) {
        snapshotCompletedSteps();
      }
    }
  }, [isStreaming]);

  // Re-fetch artifacts immediately whenever one is updated in-place
  // (i.e. the user asked to edit an existing artifact).
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    // Single app-wide listener for "artifact_updated" — it used to be
    // registered here AND in ArtifactPanel, which is mounted as a child of
    // this component. Two IPC listeners for the same event meant every
    // artifact_editor mutation and every LLM-authored artifact both fired
    // twice. This one handles both payload shapes: `{conversation_id,
    // artifact_id}` from a full artifact re-parse (chat.rs) triggers a
    // refetch, and `{source: "artifact_editor"}` from an in-place tool edit
    // (executor.rs) flashes the "AI edited" badge via the artifact store.
    listen<{ conversation_id?: string; source?: string }>("artifact_updated", (event) => {
      if (
        event.payload.conversation_id &&
        activeConversation?.id &&
        event.payload.conversation_id === activeConversation.id
      ) {
        fetchArtifacts(activeConversation.id);
      }
      if (event.payload.source === "artifact_editor") {
        flashAiEdited();
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [activeConversation?.id, fetchArtifacts, flashAiEdited]);

  // Re-fetch gallery whenever a new image is saved (e.g. from ComfyUI generation).
  // Debounced so rapid back-to-back events only trigger one fetch.
  const galleryRefreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const handleGalleryUpdated = useCallback(() => {
    if (galleryRefreshTimer.current) clearTimeout(galleryRefreshTimer.current);
    galleryRefreshTimer.current = setTimeout(() => {
      if (galleryScope === "all") {
        fetchAllImages();
      } else if (activeConversation?.id) {
        fetchImages(activeConversation.id);
      }
    }, 250);
  }, [activeConversation?.id, galleryScope, fetchImages, fetchAllImages]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ conversation_id: string }>("gallery_updated", handleGalleryUpdated)
      .then((fn) => { unlisten = fn; });
    return () => {
      unlisten?.();
      if (galleryRefreshTimer.current) clearTimeout(galleryRefreshTimer.current);
    };
  }, [handleGalleryUpdated]);

  // Stable handlers passed as props to MessageBubble — must not be recreated
  // on every render or React.memo on MessageBubble would be defeated.
  const handleEdit = useCallback(
    (id: string, content: string) => editAndResend(id, content),
    [editAndResend]
  );
  const handleRetry = useCallback(() => retryLastMessage(), [retryLastMessage]);

  // Instantly create and open a new conversation — no dialog
  const handleNewChat = async () => {
    const conv = await createConversation();
    await openConversation(conv.id);
  };

  const handleConfirmRename = async () => {
    if (!renamingId || !renameValue.trim()) { setRenamingId(null); return; }
    await renameConversation(renamingId, renameValue.trim());
    setRenamingId(null);
  };

  // Create a new conversation pre-seeded with a template's content in the input
  const handleSuggestionClick = async (content: string) => {
    const conv = await createConversation();
    await openConversation(conv.id);
    // Dispatch a custom event that InputBar listens for to pre-fill content
    window.dispatchEvent(new CustomEvent("prefill-input", { detail: { content } }));
  };

  const handleSaveSystemPrompt = async () => {
    if (!activeConversation) return;
    await updateConversation(activeConversation.id, undefined, systemPromptDraft);
    setShowSystemPrompt(false);
  };

  const handleCancelSystemPrompt = () => {
    setSystemPromptDraft(activeConversation?.system_prompt ?? "");
    setShowSystemPrompt(false);
  };

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* Conversation sidebar */}
      <div className="w-64 flex flex-col border-r border-border bg-card/50 h-full shrink-0">
        <div className="flex items-center justify-between p-3 border-b border-border">
          <span className="text-sm font-semibold text-foreground">Conversations</span>
          <Button
            size="icon"
            variant="ghost"
            className="h-7 w-7"
            onClick={handleNewChat}
            title="New conversation"
          >
            <Plus className="w-4 h-4" />
          </Button>
        </div>

        <ScrollArea className="flex-1">
          <div className="p-2 space-y-1">
            {conversations.length === 0 && (
              <button
                onClick={handleNewChat}
                className="w-full flex flex-col items-center gap-2 py-8 px-3 rounded-xl border border-dashed border-border text-muted-foreground hover:text-foreground hover:border-primary/40 hover:bg-primary/5 transition-all"
              >
                <Plus className="w-5 h-5" />
                <span className="text-xs font-medium">Start your first chat</span>
              </button>
            )}
            {conversations.map((conv) => (
              <div
                key={conv.id}
                className={cn(
                  "group flex items-center gap-2 px-2 py-1.5 rounded-lg cursor-pointer transition-colors overflow-hidden",
                  activeConversation?.id === conv.id
                    ? "bg-primary/10 text-foreground"
                    : "hover:bg-secondary text-muted-foreground hover:text-foreground"
                )}
                onClick={() => renamingId !== conv.id && openConversation(conv.id)}
              >
                <MessageSquare className="w-3.5 h-3.5 shrink-0 mt-0.5" />

                <div className="flex-1 min-w-0 overflow-hidden">
                  {renamingId === conv.id ? (
                    <input
                      ref={renameInputRef}
                      value={renameValue}
                      onChange={(e) => setRenameValue(e.target.value)}
                      onClick={(e) => e.stopPropagation()}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleConfirmRename();
                        if (e.key === "Escape") setRenamingId(null);
                      }}
                      onBlur={handleConfirmRename}
                      className="w-full text-xs bg-transparent border-b border-primary outline-none py-0.5 text-foreground"
                    />
                  ) : (
                    <>
                      <div className="text-xs font-medium truncate" title={conv.title}>{conv.title}</div>
                      <div className="text-[10px] text-muted-foreground truncate">{formatDate(conv.updated_at)}</div>
                    </>
                  )}
                </div>

                {/* Hover actions */}
                {renamingId !== conv.id && (
                  <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
                    <button
                      className="p-0.5 rounded hover:bg-secondary text-muted-foreground hover:text-foreground transition-colors"
                      title="Rename"
                      onClick={(e) => {
                        e.stopPropagation();
                        setRenameValue(conv.title);
                        setRenamingId(conv.id);
                      }}
                    >
                      <Pencil className="w-3 h-3" />
                    </button>
                    <button
                      className="p-0.5 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive transition-colors"
                      title="Delete"
                      onClick={(e) => { e.stopPropagation(); setDeleteTargetId(conv.id); }}
                    >
                      <Trash2 className="w-3 h-3" />
                    </button>
                  </div>
                )}
              </div>
            ))}
          </div>
        </ScrollArea>
      </div>

      {/* Main chat + artifact panel */}
      <div className="flex flex-1 overflow-hidden min-w-0">
        {/* Chat column */}
        <div className={cn(
          "flex flex-col h-full overflow-hidden transition-all duration-300",
          (panelOpen && artifacts.length > 0) || galleryOpen ? "flex-1 min-w-0" : "flex-1"
        )}>
          {activeConversation ? (
            <>
              {/* Header */}
              <div className="flex items-center px-4 h-14 border-b border-border shrink-0 gap-2">
                <MessageSquare className="w-4 h-4 text-primary shrink-0" />
                <span className="text-sm font-medium flex-1 truncate">{activeConversation.title}</span>
                {isStreaming && (
                  <div className="flex gap-0.5">
                    {[0, 1, 2].map((i) => (
                      <div
                        key={i}
                        className="w-1 h-1 rounded-full bg-primary animate-bounce"
                        style={{ animationDelay: `${i * 0.1}s` }}
                      />
                    ))}
                  </div>
                )}
                {/* Gallery toggle */}
                <button
                  className={cn(
                    "shrink-0 p-1.5 rounded-md transition-colors",
                    galleryOpen
                      ? "bg-violet-500/20 text-violet-400"
                      : "text-muted-foreground hover:text-foreground hover:bg-secondary"
                  )}
                  onClick={toggleGallery}
                  title="Toggle gallery"
                >
                  <Images className="w-4 h-4" />
                </button>
                {/* System prompt toggle */}
                <button
                  className={cn(
                    "shrink-0 p-1.5 rounded-md transition-colors",
                    showSystemPrompt
                      ? "bg-primary/20 text-primary"
                      : "text-muted-foreground hover:text-foreground hover:bg-secondary"
                  )}
                  onClick={() => setShowSystemPrompt((v) => !v)}
                  title="Edit system prompt"
                >
                  <Settings2 className="w-4 h-4" />
                </button>
              </div>

              {/* Collapsible system prompt editor */}
              {showSystemPrompt && (
                <div className="px-4 py-3 border-b border-border bg-card/60 flex flex-col gap-2 shrink-0">
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-xs font-medium text-muted-foreground">System Prompt</span>
                    <div className="flex items-center gap-1">
                      <button
                        className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-secondary transition-colors"
                        onClick={handleCancelSystemPrompt}
                        title="Cancel"
                      >
                        <X className="w-3.5 h-3.5" />
                      </button>
                      <button
                        className="p-1 rounded text-emerald-400 hover:text-emerald-300 hover:bg-emerald-500/10 transition-colors"
                        onClick={handleSaveSystemPrompt}
                        title="Save"
                      >
                        <Check className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>
                  <textarea
                    className="w-full text-sm bg-transparent border border-input rounded-md px-3 py-2 resize-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring min-h-[72px] max-h-[160px]"
                    placeholder="You are a helpful assistant..."
                    value={systemPromptDraft}
                    onChange={(e) => setSystemPromptDraft(e.target.value)}
                    autoFocus
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) handleSaveSystemPrompt();
                      if (e.key === "Escape") handleCancelSystemPrompt();
                    }}
                  />
                  <p className="text-[10px] text-muted-foreground">
                    Ctrl+Enter to save · Esc to cancel
                  </p>
                </div>
              )}

              {/* Messages */}
              <ScrollArea className="flex-1 overflow-x-hidden">
                <div className="w-full max-w-4xl mx-auto px-4 py-4 space-y-4">
                  {activeConversation.messages
                    .filter((m) => m.role !== "system")
                    .map((message, idx, arr) => {
                      const isLast = idx === arr.length - 1;
                      const isLastAssistant = isLast && message.role === "assistant";

                      // The streaming placeholder is rendered by its own isolated
                      // component that subscribes to streamingContent directly.
                      // This prevents all other bubbles from re-rendering on tokens.
                      if (message.id === "streaming") {
                        return (
                          <StreamingBubbleWrapper
                            key="streaming"
                            message={message}
                            activeToolSteps={activeToolSteps}
                            isStreaming={isStreaming}
                            personaAvatar={activePersona?.avatar}
                            personaName={activePersona?.name}
                            messagesEndRef={messagesEndRef}
                          />
                        );
                      }

                      // Priority order for tool steps to display:
                      // 1. Active steps while the stream is live (current turn)
                      // 2. Completed snapshot from the just-finished stream
                      // 3. Persisted steps loaded from DB (historical messages)
                      const showCompletedSteps =
                        !isStreaming && isLastAssistant && completedToolSteps.length > 0;
                      const hasPersistedSteps =
                        !isStreaming &&
                        !showCompletedSteps &&
                        message.role === "assistant" &&
                        Array.isArray(message.tool_steps) &&
                        (message.tool_steps?.length ?? 0) > 0;

                      const showToolSteps = showCompletedSteps || hasPersistedSteps;
                      const stepsToShow = showCompletedSteps
                        ? completedToolSteps
                        : (message.tool_steps ?? []);

                      return (
                        <div key={message.id} className="space-y-2">
                          {showToolSteps && (
                            <ToolCallMessage steps={stepsToShow} isStreaming={false} />
                          )}
                          <MessageBubble
                            message={message}
                            personaAvatar={activePersona?.avatar}
                            personaName={activePersona?.name}
                            onEdit={message.role === "user" && !isStreaming ? handleEdit : undefined}
                            onRegenerate={
                              isLastAssistant && !isStreaming
                                ? handleRetry
                                : undefined
                            }
                          />
                        </div>
                      );
                    })}
                  <div ref={messagesEndRef} />
                </div>
              </ScrollArea>

              {/* Input */}
              <InputBar
                collections={collections.filter((c) => c.id !== "xand_internal_memory")}
                disabled={isStreaming}
              />
            </>
          ) : (
            <div className="flex-1 flex flex-col items-center justify-center text-center p-8 gap-0">
              {/* Logo mark */}
              <div className="flex items-center justify-center w-14 h-14 rounded-2xl bg-primary/10 mb-5">
                <Zap className="w-7 h-7 text-primary" />
              </div>
              <h2 className="text-2xl font-semibold mb-2 tracking-tight">What can I help with?</h2>
              <p className="text-muted-foreground text-sm max-w-xs mb-8">
                Start a new conversation or pick a suggestion below.
              </p>

              {/* Suggestion pills from template store */}
              {suggestionTemplates.length > 0 && (
                <div className="grid grid-cols-2 gap-2 w-full max-w-sm mb-8">
                  {suggestionTemplates.map((t) => (
                    <button
                      key={t.id}
                      onClick={() => handleSuggestionClick(t.content)}
                      className={cn(
                        "flex items-start gap-2 rounded-xl border border-border bg-card px-4 py-3 text-left",
                        "text-xs text-foreground/80 hover:border-primary/40 hover:bg-primary/5 hover:text-foreground",
                        "transition-all duration-150 group"
                      )}
                    >
                      <Sparkles className="w-3.5 h-3.5 shrink-0 mt-0.5 text-primary/60 group-hover:text-primary transition-colors" />
                      <span className="font-medium leading-snug">{t.title}</span>
                    </button>
                  ))}
                </div>
              )}

              <Button onClick={handleNewChat} className="gap-2">
                <Plus className="w-4 h-4" />
                New Conversation
              </Button>
            </div>
          )}
        </div>

        {/* Right column: artifact panel and/or gallery */}
        {((panelOpen && artifacts.length > 0) || galleryOpen) && (
          <div className="w-[45%] shrink-0 h-full flex flex-col overflow-hidden border-l border-border">
            {panelOpen && artifacts.length > 0 && (
              <div
                className={cn(
                  "overflow-hidden",
                  galleryOpen ? "h-1/2 border-b border-border" : "flex-1"
                )}
              >
                <ArtifactPanel />
              </div>
            )}
            {galleryOpen && (
              <div
                className={cn(
                  "overflow-hidden",
                  panelOpen && artifacts.length > 0 ? "h-1/2" : "flex-1"
                )}
              >
                <GalleryPanel />
              </div>
            )}
          </div>
        )}
      </div>

      {/* Delete conversation confirmation */}
      <AlertDialog open={deleteTargetId !== null} onOpenChange={(open) => !open && setDeleteTargetId(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete conversation?</AlertDialogTitle>
            <AlertDialogDescription>
              This conversation and all its messages will be permanently deleted. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (deleteTargetId) deleteConversation(deleteTargetId);
                setDeleteTargetId(null);
              }}
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
