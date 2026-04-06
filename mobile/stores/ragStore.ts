import { create } from "zustand";
import { ragApi } from "../api/endpoints";
import { RagCollection } from "../lib/types";

interface RagState {
  collections: RagCollection[];
  searchResults: unknown[];
  isLoading: boolean;

  fetchCollections: () => Promise<void>;
  createCollection: (name: string, description?: string) => Promise<void>;
  deleteCollection: (id: string) => Promise<void>;
  ingest: (collection_id: string, text: string, source?: string) => Promise<void>;
  search: (query: string, collection_id?: string) => Promise<void>;
}

export const useRagStore = create<RagState>((set, get) => ({
  collections: [],
  searchResults: [],
  isLoading: false,

  fetchCollections: async () => {
    const cols = await ragApi.listCollections();
    set({ collections: cols });
  },

  createCollection: async (name, description) => {
    await ragApi.createCollection(name, description);
    await get().fetchCollections();
  },

  deleteCollection: async (id) => {
    await ragApi.deleteCollection(id);
    await get().fetchCollections();
  },

  ingest: async (collection_id, text, source) => {
    set({ isLoading: true });
    try {
      await ragApi.ingest(collection_id, text, source);
    } finally {
      set({ isLoading: false });
    }
  },

  search: async (query, collection_id) => {
    set({ isLoading: true });
    try {
      const results = await ragApi.search(query, collection_id);
      set({ searchResults: results });
    } finally {
      set({ isLoading: false });
    }
  },
}));
