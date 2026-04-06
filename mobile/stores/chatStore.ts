import { create } from "zustand";
import { chatApi } from "../api/endpoints";
import { sseManager } from "../api/sse";
import { Conversation, Message, PersistedToolStep } from "../lib/types";

interface ChatState {
  conversations: Conversation[];
  activeConversationId: string | null;
  activeConversation: Conversation | null;
  isStreaming: boolean;
  streamingToken: string;
  streamingThinking: string;

  fetchConversations: () => Promise<void>;
  selectConversation: (id: string) => Promise<void>;
  createConversation: (title?: string) => Promise<string>;
  deleteConversation: (id: string) => Promise<void>;
  updateConversation: (id: string, data: { title?: string; system_prompt?: string }) => Promise<void>;
  sendMessage: (params: {
    content: string;
    use_rag?: boolean;
    rag_collection_id?: string | null;
    use_skills?: boolean;
    attachments?: string[];
  }) => Promise<void>;
  clearActive: () => void;
}

export const useChatStore = create<ChatState>((set, get) => {
  // Subscribe to SSE events
  sseManager.on("chat_token", (event) => {
    if (event.type !== "chat_token") return;
    const { activeConversationId } = get();
    if (event.conversation_id !== activeConversationId) return;

    if (event.done) {
      set({ isStreaming: false, streamingToken: "" });
      // Reload conversation to get the saved message
      get().selectConversation(event.conversation_id);
    } else {
      set((s) => ({ streamingToken: s.streamingToken + event.token, isStreaming: true }));
    }
  });

  sseManager.on("chat_thinking", (event) => {
    if (event.type !== "chat_thinking") return;
    if (event.conversation_id !== get().activeConversationId) return;
    set((s) => ({ streamingThinking: s.streamingThinking + event.token }));
  });

  return {
    conversations: [],
    activeConversationId: null,
    activeConversation: null,
    isStreaming: false,
    streamingToken: "",
    streamingThinking: "",

    fetchConversations: async () => {
      const convs = await chatApi.listConversations();
      set({ conversations: convs });
    },

    selectConversation: async (id) => {
      set({ activeConversationId: id, streamingToken: "", streamingThinking: "" });
      sseManager.setConversationId(id);
      const conv = await chatApi.getConversation(id);
      set({ activeConversation: conv });
    },

    createConversation: async (title) => {
      const result = await chatApi.createConversation(title);
      await get().fetchConversations();
      return result.id;
    },

    deleteConversation: async (id) => {
      await chatApi.deleteConversation(id);
      if (get().activeConversationId === id) {
        set({ activeConversationId: null, activeConversation: null });
      }
      await get().fetchConversations();
    },

    updateConversation: async (id, data) => {
      await chatApi.updateConversation(id, data);
      if (get().activeConversationId === id) {
        await get().selectConversation(id);
      }
      await get().fetchConversations();
    },

    sendMessage: async ({ content, use_rag, rag_collection_id, use_skills, attachments }) => {
      const { activeConversationId } = get();
      if (!activeConversationId) return;

      set({ isStreaming: true, streamingToken: "", streamingThinking: "" });

      // Optimistically add user message to the conversation
      const userMsgId = `opt_${Date.now()}`;
      const userMsg: Message = {
        id: userMsgId,
        conversation_id: activeConversationId,
        role: "user",
        content,
        created_at: new Date().toISOString(),
      };

      set((s) => ({
        activeConversation: s.activeConversation
          ? { ...s.activeConversation, messages: [...s.activeConversation.messages, userMsg] }
          : s.activeConversation,
      }));

      try {
        const result = await chatApi.sendMessage({
          conversation_id: activeConversationId,
          content,
          use_rag,
          rag_collection_id,
          use_skills,
          attachments,
        });
        // The done:true SSE event will reload the conversation
        void result; // assistant_msg_id available if needed
      } catch (err) {
        console.error("[chat] sendMessage error:", err);
        set({ isStreaming: false });
        // Reload to remove optimistic message
        await get().selectConversation(activeConversationId);
      }
    },

    clearActive: () => {
      set({ activeConversationId: null, activeConversation: null, streamingToken: "", streamingThinking: "" });
      sseManager.setConversationId(null);
    },
  };
});
