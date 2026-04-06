import { create } from "zustand";
import { artifactApi } from "../api/endpoints";
import { sseManager } from "../api/sse";
import { Artifact } from "../lib/types";

interface ArtifactState {
  artifacts: Artifact[];
  allArtifacts: Artifact[];
  activeConversationId: string | null;

  setConversation: (id: string | null) => void;
  fetchForConversation: (id: string) => Promise<void>;
  fetchAll: () => Promise<void>;
  update: (id: string, data: Partial<Pick<Artifact, "title" | "content" | "language">>) => Promise<void>;
  delete: (id: string) => Promise<void>;
}

export const useArtifactStore = create<ArtifactState>((set, get) => {
  sseManager.on("artifact_updated", (event) => {
    if (event.type !== "artifact_updated") return;
    const { activeConversationId, fetchForConversation, fetchAll } = get();
    if (activeConversationId === event.conversation_id) {
      fetchForConversation(event.conversation_id);
    }
    fetchAll();
  });

  return {
    artifacts: [],
    allArtifacts: [],
    activeConversationId: null,

    setConversation: (id) => {
      set({ activeConversationId: id, artifacts: [] });
      if (id) get().fetchForConversation(id);
    },

    fetchForConversation: async (id) => {
      const arts = await artifactApi.list(id);
      set({ artifacts: arts });
    },

    fetchAll: async () => {
      const arts = await artifactApi.listAll();
      set({ allArtifacts: arts });
    },

    update: async (id, data) => {
      await artifactApi.update(id, data);
      const { activeConversationId, fetchForConversation, fetchAll } = get();
      if (activeConversationId) await fetchForConversation(activeConversationId);
      await fetchAll();
    },

    delete: async (id) => {
      await artifactApi.delete(id);
      const { activeConversationId, fetchForConversation, fetchAll } = get();
      if (activeConversationId) await fetchForConversation(activeConversationId);
      await fetchAll();
    },
  };
});
