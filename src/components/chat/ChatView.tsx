import { useEffect, useRef, useState } from "react";
import { Plus, Trash2, MessageSquare, Settings2, Check, X } from "lucide-react";
import { useChatStore } from "@/stores/chatStore";
import { useRagStore } from "@/stores/ragStore";
import { useSkillsStore } from "@/stores/skillsStore";
import { useArtifactStore } from "@/stores/artifactStore";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { MessageBubble } from "./MessageBubble";
import { ToolCallMessage } from "./ToolCallMessage";
import { InputBar } from "./InputBar";
import { ArtifactPanel } from "./ArtifactPanel";
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
    retryLastMessage,
    editAndResend,
  } = useChatStore();

  const { collections, fetchCollections } = useRagStore();
  const { tools, activeToolSteps, skillsEnabled, clearToolSteps, fetchTools } = useSkillsStore();
  const { artifacts, panelOpen, fetchArtifacts, clearArtifacts } = useArtifactStore();

  const messagesEndRef = useRef<HTMLDivElement>(null);

  // System prompt editor state
  const [showSystemPrompt, setShowSystemPrompt] = useState(false);
  const [systemPromptDraft, setSystemPromptDraft] = useState("");

  // Track previous streaming state to detect the transition true → false
  const prevStreamingRef = useRef(false);

  useEffect(() => {
    fetchConversations();
    fetchCollections();
    fetchTools();
  }, []);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [activeConversation?.messages]);

  // Reload artifacts whenever the active conversation changes
  useEffect(() => {
    if (activeConversation?.id) {
      clearArtifacts();
      fetchArtifacts(activeConversation.id);
    }
  }, [activeConversation?.id]);

  // Sync system prompt draft when conversation changes
  useEffect(() => {
    setSystemPromptDraft(activeConversation?.system_prompt ?? "");
    setShowSystemPrompt(false);
  }, [activeConversation?.id]);

  // Re-fetch artifacts from DB when streaming finishes.
  // The backend saves artifacts BEFORE emitting done:true, so the DB is
  // always up-to-date by the time this fires.
  useEffect(() => {
    const wasStreaming = prevStreamingRef.current;
    prevStreamingRef.current = isStreaming;
    if (wasStreaming && !isStreaming && activeConversation?.id) {
      fetchArtifacts(activeConversation.id);
    }
  }, [isStreaming]);

  // Instantly create and open a new conversation — no dialog
  const handleNewChat = async () => {
    const conv = await createConversation();
    await openConversation(conv.id);
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
              <div className="text-center text-muted-foreground text-xs py-8">
                No conversations yet.
                <br />
                Click + to start one.
              </div>
            )}
            {conversations.map((conv) => (
              <div
                key={conv.id}
                className={cn(
                  "group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-colors overflow-hidden",
                  activeConversation?.id === conv.id
                    ? "bg-primary/10 text-foreground"
                    : "hover:bg-secondary text-muted-foreground hover:text-foreground"
                )}
                onClick={() => openConversation(conv.id)}
              >
                <MessageSquare className="w-3.5 h-3.5 shrink-0" />
                <div className="flex-1 min-w-0 overflow-hidden">
                  <div className="text-xs font-medium truncate" title={conv.title}>{conv.title}</div>
                  <div className="text-[10px] text-muted-foreground truncate">{formatDate(conv.updated_at)}</div>
                </div>
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-5 w-5 opacity-0 group-hover:opacity-100 text-destructive hover:text-destructive"
                  onClick={(e) => { e.stopPropagation(); deleteConversation(conv.id); }}
                >
                  <Trash2 className="w-3 h-3" />
                </Button>
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
          panelOpen ? "flex-1 min-w-0" : "flex-1"
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
                      const showToolSteps =
                        isStreaming && isLast && message.id === "streaming" && activeToolSteps.length > 0;
                      return (
                        <div key={message.id} className="space-y-2">
                          {showToolSteps && (
                            <ToolCallMessage steps={activeToolSteps} isStreaming={isStreaming} />
                          )}
                          <MessageBubble
                            message={message}
                            liveThinking={message.id === "streaming" ? streamingThinking : undefined}
                            isThinking={message.id === "streaming" ? isThinking : undefined}
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
              <InputBar collections={collections} disabled={isStreaming} />
            </>
          ) : (
            <div className="flex-1 flex flex-col items-center justify-center text-center p-8">
              <div className="w-16 h-16 rounded-2xl bg-primary/10 flex items-center justify-center mb-4">
                <MessageSquare className="w-8 h-8 text-primary" />
              </div>
              <h2 className="text-xl font-semibold mb-2">XandSuite Chat</h2>
              <p className="text-muted-foreground text-sm max-w-sm mb-6">
                Select a conversation or create a new one to start chatting with your local AI models.
              </p>
              <Button onClick={handleNewChat}>
                <Plus className="w-4 h-4 mr-2" />
                New Conversation
              </Button>
            </div>
          )}
        </div>

        {/* Artifact panel (slide in from right) */}
        {panelOpen && artifacts.length > 0 && (
          <div className="w-[45%] shrink-0 h-full overflow-hidden border-l border-border">
            <ArtifactPanel />
          </div>
        )}
      </div>
    </div>
  );
}
