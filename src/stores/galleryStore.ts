import { create } from "zustand";
import { invoke } from "@/lib/tauri";
import type { GalleryImage } from "@/lib/tauri";

interface GalleryStore {
  images: GalleryImage[];
  scope: "conversation" | "all";
  galleryOpen: boolean;
  activeConversationId: string | null;

  fetchImages: (conversationId: string) => Promise<void>;
  fetchAllImages: () => Promise<void>;
  refresh: () => Promise<void>;
  deleteImage: (id: string) => Promise<void>;
  saveUpload: (
    conversationId: string,
    filename: string,
    imageData: string,
    mimeType: string
  ) => Promise<void>;
  openGallery: () => void;
  closeGallery: () => void;
  toggleGallery: () => void;
  setScope: (scope: "conversation" | "all") => void;
  setActiveConversation: (id: string | null) => void;
}

export const useGalleryStore = create<GalleryStore>((set, get) => ({
  images: [],
  scope: "conversation",
  galleryOpen: false,
  activeConversationId: null,

  fetchImages: async (conversationId: string) => {
    try {
      const images = await invoke<GalleryImage[]>("list_gallery_images", {
        conversationId,
      });
      set({ images, activeConversationId: conversationId });
    } catch (e) {
      console.error("Failed to fetch gallery images:", e);
    }
  },

  fetchAllImages: async () => {
    try {
      const images = await invoke<GalleryImage[]>("list_all_gallery_images");
      set({ images });
    } catch (e) {
      console.error("Failed to fetch all gallery images:", e);
    }
  },

  refresh: async () => {
    const { scope, activeConversationId, fetchImages, fetchAllImages } = get();
    if (scope === "all") {
      await fetchAllImages();
    } else if (activeConversationId) {
      await fetchImages(activeConversationId);
    }
  },

  deleteImage: async (id: string) => {
    try {
      await invoke("delete_gallery_image", { id });
      set((s) => ({ images: s.images.filter((img) => img.id !== id) }));
    } catch (e) {
      console.error("Failed to delete gallery image:", e);
    }
  },

  saveUpload: async (
    conversationId: string,
    filename: string,
    imageData: string,
    mimeType: string
  ) => {
    try {
      const image = await invoke<GalleryImage>("save_upload_to_gallery", {
        payload: { conversation_id: conversationId, filename, image_data: imageData, mime_type: mimeType },
      });
      set((s) => ({ images: [...s.images, image] }));
    } catch (e) {
      console.error("Failed to save upload to gallery:", e);
    }
  },

  openGallery: () => set({ galleryOpen: true }),
  closeGallery: () => set({ galleryOpen: false }),
  toggleGallery: () => set((s) => ({ galleryOpen: !s.galleryOpen })),

  setScope: (scope) => {
    set({ scope });
    const { activeConversationId, fetchImages, fetchAllImages } = get();
    if (scope === "all") {
      fetchAllImages();
    } else if (activeConversationId) {
      fetchImages(activeConversationId);
    }
  },

  setActiveConversation: (id) => set({ activeConversationId: id }),
}));
