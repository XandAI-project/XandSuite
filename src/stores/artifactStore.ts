import { create } from "zustand";
import { invoke } from "@/lib/tauri";
import type { Artifact, ArtifactType } from "@/lib/tauri";

interface SaveArtifactParams {
  conversationId: string;
  messageId?: string;
  title: string;
  artifactType: ArtifactType;
  language?: string;
  content: string;
}

interface ArtifactStore {
  artifacts: Artifact[];
  activeArtifactId: string | null;
  panelOpen: boolean;
  /** True for ~3s right after an `artifact_editor` tool call mutates the
   *  active artifact, so ArtifactPanel can flash an "AI edited" badge. */
  aiEdited: boolean;

  fetchArtifacts: (conversationId: string) => Promise<void>;
  fetchAllArtifacts: () => Promise<Artifact[]>;
  saveArtifact: (params: SaveArtifactParams) => Promise<Artifact>;
  deleteArtifact: (id: string) => Promise<void>;
  updateArtifact: (id: string, title: string, content: string) => Promise<void>;
  openArtifact: (id: string) => void;
  /** Remove an artifact from the panel without deleting it from the database. */
  dismissArtifact: (id: string) => void;
  closePanel: () => void;
  clearArtifacts: () => void;
  /** Briefly set `aiEdited`, auto-clearing after 3s. */
  flashAiEdited: () => void;
}

// Module-scoped so a rapid run of artifact_editor calls resets the same
// timer instead of racing multiple independent setTimeout callbacks.
let aiEditedTimer: ReturnType<typeof setTimeout> | null = null;

export const useArtifactStore = create<ArtifactStore>((set) => ({
  artifacts: [],
  activeArtifactId: null,
  panelOpen: false,
  aiEdited: false,

  fetchArtifacts: async (conversationId: string) => {
    try {
      const artifacts = await invoke<Artifact[]>("list_artifacts", { conversationId });
      set({ artifacts });
    } catch (e) {
      console.error("Failed to fetch artifacts:", e);
    }
  },

  fetchAllArtifacts: async () => {
    try {
      return await invoke<Artifact[]>("list_all_artifacts");
    } catch (e) {
      console.error("Failed to fetch all artifacts:", e);
      return [];
    }
  },

  saveArtifact: async (params: SaveArtifactParams) => {
    const artifact = await invoke<Artifact>("save_artifact", {
      conversationId: params.conversationId,
      messageId: params.messageId ?? null,
      title: params.title,
      artifactType: params.artifactType,
      language: params.language ?? null,
      content: params.content,
    });
    set((s) => ({ artifacts: [...s.artifacts, artifact] }));
    return artifact;
  },

  deleteArtifact: async (id: string) => {
    await invoke("delete_artifact", { id });
    set((s) => {
      const artifacts = s.artifacts.filter((a) => a.id !== id);
      const wasActive = s.activeArtifactId === id;
      const nextActive = wasActive ? (artifacts[artifacts.length - 1]?.id ?? null) : s.activeArtifactId;
      return {
        artifacts,
        activeArtifactId: nextActive,
        panelOpen: artifacts.length > 0 ? s.panelOpen : false,
      };
    });
  },

  updateArtifact: async (id: string, title: string, content: string) => {
    const updated = await invoke<Artifact>("update_artifact", { id, title, content });
    set((s) => ({
      artifacts: s.artifacts.map((a) => (a.id === id ? updated : a)),
    }));
  },

  openArtifact: (id: string) => {
    set({ activeArtifactId: id, panelOpen: true });
  },

  dismissArtifact: (id: string) => {
    set((s) => {
      const artifacts = s.artifacts.filter((a) => a.id !== id);
      const wasActive = s.activeArtifactId === id;
      const nextActive = wasActive
        ? (artifacts[artifacts.length - 1]?.id ?? null)
        : s.activeArtifactId;
      return {
        artifacts,
        activeArtifactId: nextActive,
        panelOpen: artifacts.length > 0 ? s.panelOpen : false,
      };
    });
  },

  closePanel: () => {
    set({ panelOpen: false });
  },

  clearArtifacts: () => {
    // Don't touch panelOpen — ChatView's `artifacts.length > 0` guard hides the
    // panel while the list is empty, and it reappears automatically once the
    // new conversation's artifacts are loaded.
    set({ artifacts: [], activeArtifactId: null });
  },

  flashAiEdited: () => {
    if (aiEditedTimer) clearTimeout(aiEditedTimer);
    set({ aiEdited: true });
    aiEditedTimer = setTimeout(() => {
      aiEditedTimer = null;
      set({ aiEdited: false });
    }, 3000);
  },
}));
