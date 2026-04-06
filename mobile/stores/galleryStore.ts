import { create } from "zustand";
import { galleryApi } from "../api/endpoints";
import { sseManager } from "../api/sse";
import { GalleryImage } from "../lib/types";

interface GalleryState {
  images: GalleryImage[];
  allImages: GalleryImage[];
  activeConversationId: string | null;
  scope: "conversation" | "all";

  setConversation: (id: string | null) => void;
  setScope: (scope: "conversation" | "all") => void;
  fetchImages: (conversation_id: string) => Promise<void>;
  fetchAllImages: () => Promise<void>;
  refresh: () => Promise<void>;
  deleteImage: (id: string) => Promise<void>;
}

export const useGalleryStore = create<GalleryState>((set, get) => {
  sseManager.on("gallery_updated", (event) => {
    if (event.type !== "gallery_updated") return;
    get().refresh();
  });

  return {
    images: [],
    allImages: [],
    activeConversationId: null,
    scope: "all",

    setConversation: (id) => {
      set({ activeConversationId: id });
      if (id) get().fetchImages(id);
    },

    setScope: (scope) => set({ scope }),

    fetchImages: async (conversation_id) => {
      const imgs = await galleryApi.list(conversation_id);
      set({ images: imgs });
    },

    fetchAllImages: async () => {
      const imgs = await galleryApi.listAll();
      set({ allImages: imgs });
    },

    refresh: async () => {
      const { activeConversationId, fetchImages, fetchAllImages } = get();
      if (activeConversationId) await fetchImages(activeConversationId);
      await fetchAllImages();
    },

    deleteImage: async (id) => {
      await galleryApi.delete(id);
      await get().refresh();
    },
  };
});
