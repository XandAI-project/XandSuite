import { create } from "zustand";
import { invoke } from "../lib/tauri";
import type { RagCollection } from "../lib/tauri";

interface RagStore {
  collections: RagCollection[];
  isLoading: boolean;
  error: string | null;

  fetchCollections: () => Promise<void>;
  createCollection: (name: string, description?: string) => Promise<void>;
  deleteCollection: (id: string) => Promise<void>;
  ingestDocument: (collectionId: string, filePath: string) => Promise<void>;
}

export const useRagStore = create<RagStore>((set, _get) => ({
  collections: [],
  isLoading: false,
  error: null,

  fetchCollections: async () => {
    try {
      const collections = await invoke<RagCollection[]>("list_rag_collections");
      set({ collections });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  createCollection: async (name: string, description?: string) => {
    try {
      await invoke("create_rag_collection", { name, description: description || null });
      const collections = await invoke<RagCollection[]>("list_rag_collections");
      set({ collections });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  deleteCollection: async (id: string) => {
    try {
      await invoke("delete_rag_collection", { collectionId: id });
      const collections = await invoke<RagCollection[]>("list_rag_collections");
      set({ collections });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  ingestDocument: async (collectionId: string, filePath: string) => {
    set({ isLoading: true });
    try {
      await invoke("ingest_document", { collectionId, filePath });
      const collections = await invoke<RagCollection[]>("list_rag_collections");
      set({ collections, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },
}));
