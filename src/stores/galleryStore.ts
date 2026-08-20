import { create } from "zustand";
import { invoke } from "@/lib/tauri";
import type { GalleryImage } from "@/lib/tauri";

interface GalleryStore {
  images: GalleryImage[];
  scope: "conversation" | "all";
  galleryOpen: boolean;
  activeConversationId: string | null;
  /** True once at least one successful fetch has completed — used to suppress false EmptyState flash */
  isInitialized: boolean;

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

// Tracks in-flight fetch keys so we don't fire the same query twice concurrently.
// "all" is the key for fetchAllImages; a conversationId string is the key for fetchImages.
const _inFlight = new Set<string>();

/** Fields that affect how an image tile renders. */
function sameImage(a: GalleryImage, b: GalleryImage): boolean {
  return (
    a.id === b.id &&
    a.file_path === b.file_path &&
    a.image_data === b.image_data &&
    a.mime_type === b.mime_type &&
    a.source === b.source
  );
}

/**
 * Merge a freshly-fetched image list into the previous one while preserving the
 * object identity of unchanged entries. Returning the same references lets the
 * memoized `ImageTile`s skip re-rendering, which is what kills the flicker when
 * a new image arrives and the whole list is refetched. If nothing changed, the
 * previous array reference is returned so Zustand skips the update entirely.
 */
function mergeImages(prev: GalleryImage[], next: GalleryImage[]): GalleryImage[] {
  const prevById = new Map(prev.map((img) => [img.id, img]));
  let changed = prev.length !== next.length;
  const merged = next.map((n) => {
    const old = prevById.get(n.id);
    if (old && sameImage(old, n)) return old; // reuse reference
    changed = true;
    return n;
  });
  return changed ? merged : prev;
}

export const useGalleryStore = create<GalleryStore>((set, get) => ({
  images: [],
  scope: "conversation",
  galleryOpen: false,
  activeConversationId: null,
  isInitialized: false,

  fetchImages: async (conversationId: string) => {
    const key = conversationId;
    if (_inFlight.has(key)) return;
    _inFlight.add(key);
    try {
      const fetched = await invoke<GalleryImage[]>("list_gallery_images", {
        conversationId,
      });
      set((s) => {
        // Guard against a cross-conversation race: if the user switched to a
        // different conversation (or to the "all" scope) while this fetch was
        // in flight, `activeConversationId` no longer matches the id we
        // requested. Applying this response anyway would silently repaint the
        // gallery with the wrong conversation's images and stamp
        // `activeConversationId` back to the stale id.
        if (s.activeConversationId !== conversationId) return s;
        return {
          images: mergeImages(s.images, fetched),
          activeConversationId: conversationId,
          isInitialized: true,
        };
      });
    } catch (e) {
      console.error("Failed to fetch gallery images:", e);
    } finally {
      _inFlight.delete(key);
    }
  },

  fetchAllImages: async () => {
    const key = "all";
    if (_inFlight.has(key)) return;
    _inFlight.add(key);
    try {
      const fetched = await invoke<GalleryImage[]>("list_all_gallery_images");
      set((s) => {
        // Same staleness guard as fetchImages: if the scope switched back to
        // "conversation" while this request was in flight, don't clobber the
        // per-conversation images that may have loaded in the meantime.
        if (s.scope !== "all") return s;
        return { images: mergeImages(s.images, fetched), isInitialized: true };
      });
    } catch (e) {
      console.error("Failed to fetch all gallery images:", e);
    } finally {
      _inFlight.delete(key);
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
    // Keep `isInitialized` true so the existing grid stays on screen while the
    // new scope loads — flipping it to false would flash the loading spinner.
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
