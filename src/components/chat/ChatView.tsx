import { useEffect, useMemo, useRef, useState } from "react";
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

export function ChatView() {
  const {
    conversations,
    activeConversation,
    isStreaming,
    isThinking,
    streamingThinking,
    fetchConversations,
    openConversation,
    createConversation,
    updateConversation,
    deleteConversation,
    renameConversation,
    retryLastMessage,
    editAndResend,
  } = useChatStore();

  const { collections, fetchCollections } = useRagStore();
  const {
    activeToolSteps,
    completedToolSteps,
    snapshotCompletedSteps,
    clearToolSteps,
    fetchTools,
  } = useSkillsStore();
  const { artifacts, panelOpen, fetchArtifacts, clearArtifacts } = useArtifactStore();
  const {
    galleryOpen,
    toggleGallery,
    fetchImages,
    fetchAllImages,
    scope: galleryScope,
    setActiveConversation: setGalleryConversation,
  } = useGalleryStore();

  const { personas, fetchPersonas } = usePersonaStore();
  const { templates, fetchTemplates } = useTemplateStore();

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

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [activeConversation?.messages]);

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
    let unlisten: (() => void) | undefined;
    listen<{ conversation_id: string }>("artifact_updated", (event) => {
      if (
        activeConversation?.id &&
        event.payload.conversation_id === activeConversation.id
      ) {
        fetchArtifacts(activeConversation.id);
      }
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [activeConversation?.id]);

  // Re-fetch gallery whenever a new image is saved (e.g. from ComfyUI generation)
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ conversation_id: string }>("gallery_updated", () => {
      if (galleryScope === "all") {
        fetchAllImages();
      } else if (activeConversation?.id) {
        fetchImages(activeConversation.id);
      }
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [activeConversation?.id, galleryScope]);

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
              <ScrollArea className="flex-1 px-4 py-4">
                <div className="max-w-3xl mx-auto space-y-4">
                  {activeConversation.messages
                    .filter((m) => m.role !== "system")
                    .map((message, idx, arr) => {
                      const isLast = idx === arr.length - 1;
                      const isLastAssistant = isLast && message.role === "assistant";

                      // Priority order for tool steps to display:
                      // 1. Active steps while the stream is live (current turn)
                      // 2. Completed snapshot from the just-finished stream
                      // 3. Persisted steps loaded from DB (historical messages)
                      const showActiveSteps =
                        isStreaming && isLast && message.id === "streaming" && activeToolSteps.length > 0;
                      const showCompletedSteps =
                        !isStreaming && isLastAssistant && completedToolSteps.length > 0;
                      const hasPersistedSteps =
                        !isStreaming &&
                        !showCompletedSteps &&
                        message.role === "assistant" &&
                        Array.isArray(message.tool_steps) &&
                        (message.tool_steps?.length ?? 0) > 0;

                      const showToolSteps = showActiveSteps || showCompletedSteps || hasPersistedSteps;
                      const stepsToShow = showActiveSteps
                        ? activeToolSteps
                        : showCompletedSteps
                        ? completedToolSteps
                        : (message.tool_steps ?? []);

                      return (
                        <div key={message.id} className="space-y-2">
                          {showToolSteps && (
                            <ToolCallMessage steps={stepsToShow} isStreaming={isStreaming && showActiveSteps} />
                          )}
                          <MessageBubble
                            message={message}
                            liveThinking={message.id === "streaming" ? streamingThinking : undefined}
                            isThinking={message.id === "streaming" ? isThinking : undefined}
                            personaAvatar={activePersona?.avatar}
                            personaName={activePersona?.name}
                            onEdit={
                              message.role === "user" && !isStreaming
                                ? (id, content) => editAndResend(id, content)
                                : undefined
                            }
                            onRegenerate={
                              message.role === "assistant" && isLast && !isStreaming && message.id !== "streaming"
                                ? retryLastMessage
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
