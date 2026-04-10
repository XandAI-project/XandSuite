import { create } from "zustand";
import { invoke } from "../lib/tauri";
import { listen } from "@tauri-apps/api/event";
import type { Conversation, ConversationSummary, Message, ImageMeta } from "../lib/tauri";
import { useSkillsStore } from "./skillsStore";
import { readFile } from "@tauri-apps/plugin-fs";

interface ChatStore {
  conversations: ConversationSummary[];
  activeConversation: Conversation | null;
  streamingContent: string;
  /** Accumulated reasoning/thinking content for the current streaming message */
  streamingThinking: string;
  /** Whether the model is currently in its thinking phase */
  isThinking: boolean;
  isStreaming: boolean;
  /** Which conversation id is currently receiving a stream (null when idle). */
  streamingConversationId: string | null;
  isLoading: boolean;
  error: string | null;

  fetchConversations: () => Promise<void>;
  openConversation: (id: string) => Promise<void>;
  /** Create a new conversation with an auto-generated "New chat" title. */
  createConversation: (systemPrompt?: string, personaId?: string) => Promise<Conversation>;
  /** Update the title and/or system prompt of a conversation. */
  updateConversation: (id: string, title?: string, systemPrompt?: string) => Promise<void>;
  /** Rename a conversation (inline rename in sidebar). */
  renameConversation: (id: string, title: string) => Promise<void>;
  deleteConversation: (id: string) => Promise<void>;
  sendMessage: (content: string, useRag?: boolean, ragCollectionId?: string, useSkills?: boolean, attachments?: string[]) => Promise<void>;
  /** Abort the currently active generation. */
  stopGeneration: () => Promise<void>;
  /** Remove the last assistant reply and re-send the user message that preceded it. */
  retryLastMessage: () => Promise<void>;
  /** Truncate the conversation from `messageId` onwards and re-send with `newContent`. */
  editAndResend: (messageId: string, newContent: string) => Promise<void>;
  clearError: () => void;
}

export const useChatStore = create<ChatStore>((set, get) => ({
  conversations: [],
  activeConversation: null,
  streamingContent: "",
  streamingThinking: "",
  isThinking: false,
  isStreaming: false,
  streamingConversationId: null,
  isLoading: false,
  error: null,

  fetchConversations: async () => {
    try {
      const conversations = await invoke<ConversationSummary[]>("list_conversations");
      set({ conversations });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  openConversation: async (id: string) => {
    set({ isLoading: true, error: null });
    try {
      const conv = await invoke<Conversation>("get_conversation", { conversationId: id });

      // If we're in the middle of streaming for this exact conversation, the
      // assistant message exists only in memory (not in DB yet).  Re-append it
      // so the user sees the live response after navigating away and back.
      const state = get();
      if (state.isStreaming && state.streamingConversationId === id) {
        const liveMsg: Message = {
          id: "streaming",
          conversation_id: id,
          role: "assistant",
          content: state.streamingContent,
          created_at: new Date().toISOString(),
        };
        set({
          activeConversation: { ...conv, messages: [...conv.messages, liveMsg] },
          isLoading: false,
        });
      } else {
        set({ activeConversation: conv, isLoading: false });
      }
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  createConversation: async (systemPrompt?: string, personaId?: string) => {
    const conv = await invoke<Conversation>("create_conversation", {
      title: "New chat",
      systemPrompt: systemPrompt || null,
      personaId: personaId || null,
    });
    await get().fetchConversations();
    return conv;
  },

  updateConversation: async (id: string, title?: string, systemPrompt?: string) => {
    await invoke("update_conversation", {
      conversationId: id,
      title: title ?? null,
      systemPrompt: systemPrompt ?? null,
    });
    await get().fetchConversations();
    // Refresh the active conversation if it's the one being updated
    if (get().activeConversation?.id === id) {
      await get().openConversation(id);
    }
  },

  renameConversation: async (id: string, title: string) => {
    await invoke("rename_conversation", { conversationId: id, title });
    set((state) => ({
      conversations: state.conversations.map((c) =>
        c.id === id ? { ...c, title } : c
      ),
      activeConversation:
        state.activeConversation?.id === id
          ? { ...state.activeConversation, title }
          : state.activeConversation,
    }));
  },

  deleteConversation: async (id: string) => {
    await invoke("delete_conversation", { conversationId: id });
    const active = get().activeConversation;
    if (active?.id === id) {
      set({ activeConversation: null });
    }
    await get().fetchConversations();
  },

  sendMessage: async (content: string, useRag = false, ragCollectionId?: string, useSkills = false, attachments?: string[]) => {
    const conv = get().activeConversation;
    if (!conv) return;

    // Count non-system messages before this send so we know if it's the first exchange
    const prevUserMsgCount = conv.messages.filter((m) => m.role === "user").length;

    const imageExts = new Set(["jpg", "jpeg", "png", "gif", "webp", "bmp"]);
    const isImage = (p: string) => {
      const ext = p.replace(/\\/g, "/").split(".").pop()?.toLowerCase() ?? "";
      return imageExts.has(ext);
    };

    // Separate image paths from text attachments.
    const imagePaths = attachments?.filter(isImage) ?? [];
    const textAttachments = attachments?.filter((p) => !isImage(p)) ?? [];
    const attachmentNames = textAttachments.map((p) => p.replace(/\\/g, "/").split("/").pop() ?? p);

    const metaAttachments = attachmentNames.length ? attachmentNames : undefined;

    // Read image files as base64 for immediate display and DB persistence.
    const extToMime: Record<string, string> = {
      jpg: "image/jpeg", jpeg: "image/jpeg", png: "image/png",
      gif: "image/gif", webp: "image/webp", bmp: "image/bmp",
    };
    let metaImages: ImageMeta[] | undefined;
    if (imagePaths.length > 0) {
      const results = await Promise.all(
        imagePaths.map(async (p): Promise<ImageMeta | null> => {
          try {
            const bytes = await readFile(p);
            const ext = p.replace(/\\/g, "/").split(".").pop()?.toLowerCase() ?? "jpeg";
            const mime = extToMime[ext] ?? "image/jpeg";
            // Convert Uint8Array → base64 without btoa (handles large files)
            let b64 = "";
            const CHUNK = 8192;
            const arr = new Uint8Array(bytes);
            for (let i = 0; i < arr.length; i += CHUNK) {
              b64 += String.fromCharCode(...arr.subarray(i, i + CHUNK));
            }
            b64 = btoa(b64);
            const filename = p.replace(/\\/g, "/").split("/").pop() ?? p;
            return { filename, mime, data: b64 };
          } catch {
            return null;
          }
        })
      );
      const valid = results.filter((r): r is ImageMeta => r !== null);
      if (valid.length > 0) metaImages = valid;
    }

    const userMsg: Message = {
      id: crypto.randomUUID(),
      conversation_id: conv.id,
      role: "user",
      content,
      created_at: new Date().toISOString(),
      metadata: (metaAttachments || metaImages)
        ? { attachments: metaAttachments, images: metaImages }
        : undefined,
    };

    const assistantMsg: Message = {
      id: "streaming",
      conversation_id: conv.id,
      role: "assistant",
      content: "",
      created_at: new Date().toISOString(),
    };

    set((state) => ({
      activeConversation: state.activeConversation
        ? {
            ...state.activeConversation,
            // Scrub any orphaned streaming placeholder from a previous aborted
            // generation before appending the new messages, preventing ghost
            // messages and duplicate-looking entries in the chat.
            messages: [
              ...state.activeConversation.messages.filter((m) => m.id !== "streaming"),
              userMsg,
              assistantMsg,
            ],
          }
        : null,
      isStreaming: true,
      isThinking: false,
      streamingContent: "",
      streamingThinking: "",
      streamingConversationId: conv.id,
    }));

    type TokenPayload = { conversation_id: string; token: string; done: boolean };
    type ThinkPayload = { conversation_id: string; token: string };
    type ThinkClearPayload = { conversation_id: string };

    let unlistenToken: (() => void) | undefined;
    let unlistenThink: (() => void) | undefined;
    let unlistenThinkClear: (() => void) | undefined;

    // Timing / throughput tracking
    let firstTokenAt: number | null = null;
    let tokenCount = 0;

    // Register listeners BEFORE invoking the backend so no early events are dropped.
    // Thinking tokens
    unlistenThink = await listen<ThinkPayload>("chat_thinking", (event) => {
      const p = event.payload;
      if (p.conversation_id !== conv.id) return;
      set((s) => ({
        streamingThinking: s.streamingThinking + p.token,
        isThinking: true,
      }));
    });

    // Thinking clear — backend promoted thinking content to the response body.
    // Discard accumulated thinking so the reasoning block is not shown.
    unlistenThinkClear = await listen<ThinkClearPayload>("chat_thinking_clear", (event) => {
      if (event.payload.conversation_id !== conv.id) return;
      set({ streamingThinking: "", isThinking: false });
    });

    // Response tokens
    unlistenToken = await listen<TokenPayload>("chat_token", (event) => {
      const p = event.payload;
      if (p.conversation_id !== conv.id) return;

      if (p.done) {
        const finalContent = get().streamingContent;
        const finalThinking = get().streamingThinking;

        // Calculate tokens/s from first visible token to done
        const tps =
          firstTokenAt !== null && tokenCount > 0
            ? parseFloat((tokenCount / ((Date.now() - firstTokenAt) / 1000)).toFixed(1))
            : null;

        // Determine if the user is still viewing the conversation that just finished.
        // If they navigated away, activeConversation is a different conversation and
        // the .map() below would find no "streaming" message — the final content would
        // be silently dropped.
        const isStillActive = get().activeConversation?.id === conv.id;
        const hasStreamingMsg = get().activeConversation?.messages.some((m) => m.id === "streaming") ?? false;

        if (isStillActive && hasStreamingMsg) {
          // Normal path: user stayed on this conversation — patch the streaming message.
          set((state) => ({
            activeConversation: state.activeConversation
              ? {
                  ...state.activeConversation,
                  messages: state.activeConversation.messages.map((m) =>
                    m.id === "streaming"
                      ? {
                          ...m,
                          content: finalContent,
                          thinking: finalThinking || undefined,
                          tps: tps ?? undefined,
                          id: crypto.randomUUID(),
                        }
                      : m
                  ),
                }
              : null,
            isStreaming: false,
            isThinking: false,
            streamingContent: "",
            streamingThinking: "",
            streamingConversationId: null,
          }));
        } else {
          // The user navigated away during streaming. The backend has already
          // persisted the final message. Clear streaming state and re-fetch
          // the conversation from DB so the final content is visible.
          set({
            isStreaming: false,
            isThinking: false,
            streamingContent: "",
            streamingThinking: "",
            streamingConversationId: null,
          });
          // If they navigated back to this conversation (or are still on it but
          // without the streaming placeholder), reload it from DB.
          if (get().activeConversation?.id === conv.id) {
            get().openConversation(conv.id);
          }
        }

        unlistenToken?.();
        unlistenThink?.();
        unlistenThinkClear?.();

        // Auto-title: rename the conversation on the first exchange.
        if (prevUserMsgCount === 0 && conv.title === "New chat") {
          const autoTitle = content.trim().slice(0, 25) + (content.trim().length > 25 ? "…" : "");
          invoke("update_conversation", {
            conversationId: conv.id,
            title: autoTitle,
            systemPrompt: null,
          }).finally(() => get().fetchConversations());
        } else {
          get().fetchConversations();
        }
      } else {
        if (firstTokenAt === null) firstTokenAt = Date.now();
        // Count tokens roughly by whitespace-split (fast, good-enough approximation)
        tokenCount += p.token.split(/\s+/).filter(Boolean).length || 1;

        set((state) => ({
          streamingContent: state.streamingContent + p.token,
          isThinking: false,
          activeConversation: state.activeConversation
            ? {
                ...state.activeConversation,
                messages: state.activeConversation.messages.map((m) =>
                  m.id === "streaming"
                    ? { ...m, content: state.streamingContent + p.token }
                    : m
                ),
              }
            : null,
        }));
      }
    });

    try {
      const assistantMsgId = await invoke<string>("send_message", {
        conversationId: conv.id,
        content,
        useRag,
        ragCollectionId: ragCollectionId || null,
        useSkills,
        attachments: attachments ?? [],
      });

      // Persist any tool steps that were collected during this response.
      // By the time invoke() resolves, the done:true event has already fired,
      // so activeToolSteps is fully populated.
      const { activeToolSteps } = useSkillsStore.getState();
      if (activeToolSteps.length > 0 && assistantMsgId) {
        invoke("save_message_tool_steps", {
          messageId: assistantMsgId,
          toolStepsJson: JSON.stringify(activeToolSteps),
        }).catch((e) => console.warn("Failed to save tool steps:", e));
      }
    } catch (e) {
      set((s) => ({
        activeConversation: s.activeConversation
          ? {
              ...s.activeConversation,
              messages: s.activeConversation.messages.filter((m) => m.id !== "streaming"),
            }
          : null,
        isStreaming: false,
        isThinking: false,
        streamingContent: "",
        streamingThinking: "",
        streamingConversationId: null,
        error: String(e),
      }));
      unlistenToken?.();
      unlistenThink?.();
      unlistenThinkClear?.();
    }
  },

  retryLastMessage: async () => {
    const conv = get().activeConversation;
    if (!conv || get().isStreaming) return;

    const visible = conv.messages.filter((m) => m.role !== "system");
    // Find the last assistant message and the user message right before it
    const lastAssistIdx = [...visible].reverse().findIndex((m) => m.role === "assistant");
    if (lastAssistIdx === -1) return;
    const userMsg = visible[visible.length - 2 - lastAssistIdx];
    if (!userMsg || userMsg.role !== "user") return;

    // Truncate DB from the user message (removes both user + assistant)
    await invoke("truncate_conversation", {
      conversationId: conv.id,
      fromMessageId: userMsg.id,
    });

    // Truncate local state
    const cutIdx = conv.messages.findIndex((m) => m.id === userMsg.id);
    set((state) => ({
      activeConversation: state.activeConversation
        ? { ...state.activeConversation, messages: state.activeConversation.messages.slice(0, cutIdx) }
        : null,
    }));

    await get().sendMessage(userMsg.content);
  },

  editAndResend: async (messageId: string, newContent: string) => {
    const conv = get().activeConversation;
    if (!conv || get().isStreaming) return;

    // Truncate DB from this message onwards
    await invoke("truncate_conversation", {
      conversationId: conv.id,
      fromMessageId: messageId,
    });

    // Truncate local state up to (not including) this message
    const cutIdx = conv.messages.findIndex((m) => m.id === messageId);
    if (cutIdx === -1) return;
    set((state) => ({
      activeConversation: state.activeConversation
        ? { ...state.activeConversation, messages: state.activeConversation.messages.slice(0, cutIdx) }
        : null,
    }));

    await get().sendMessage(newContent);
  },

  clearError: () => set({ error: null }),

  stopGeneration: async () => {
    await invoke("stop_generation");
    // Immediately unblock the UI — don't wait for the backend [DONE] event.
    // The backend will still send [DONE] eventually; when it arrives the
    // done-handler will see no "streaming" message and reload from DB so
    // any partial content that was persisted shows up correctly.
    set((s) => ({
      activeConversation: s.activeConversation
        ? {
            ...s.activeConversation,
            messages: s.activeConversation.messages.filter((m) => m.id !== "streaming"),
          }
        : null,
      isStreaming: false,
      isThinking: false,
      streamingContent: "",
      streamingThinking: "",
      streamingConversationId: null,
    }));
  },
}));
