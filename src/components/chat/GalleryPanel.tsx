import { useRef, useEffect, useState, useCallback, useMemo, memo } from "react";
import {
  X,
  Trash2,
  Download,
  Upload,
  Images,
  ChevronLeft,
  ChevronRight,
  ImageOff,
  Loader2,
} from "lucide-react";
import { useGalleryStore } from "@/stores/galleryStore";
import type { GalleryImage } from "@/lib/tauri";
import { cn, resolveGallerySrc } from "@/lib/utils";

// ── Helpers ───────────────────────────────────────────────────────────────────

function isVideo(img: GalleryImage): boolean {
  return img.mime_type.startsWith("video/");
}

function mediaSrc(img: GalleryImage): string {
  if (isVideo(img)) {
    return img.image_data;
  }
  return resolveGallerySrc(img) || img.image_data;
}

// imgSrc alias removed — use mediaSrc directly

function formatDate(iso: string): string {
  try {
    return new Intl.DateTimeFormat(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(iso));
  } catch {
    return iso;
  }
}

// ── Lightbox ──────────────────────────────────────────────────────────────────

function Lightbox({
  images,
  startIndex,
  onClose,
}: {
  images: GalleryImage[];
  startIndex: number;
  onClose: () => void;
}) {
  const [idx, setIdx] = useState(startIndex);
  const img = images[idx];
  const currentSrc = useMemo(() => mediaSrc(img), [img?.id, img?.file_path, img?.image_data]);

  const prev = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      setIdx((i) => Math.max(0, i - 1));
    },
    []
  );

  const next = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      setIdx((i) => Math.min(images.length - 1, i + 1));
    },
    [images.length]
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "ArrowLeft") setIdx((i) => Math.max(0, i - 1));
      if (e.key === "ArrowRight") setIdx((i) => Math.min(images.length - 1, i + 1));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, images.length]);

  const handleDownload = () => {
    if (isVideo(img)) {
      window.open(currentSrc, "_blank");
    } else {
      const a = document.createElement("a");
      a.href = currentSrc;
      a.download = img.filename;
      a.click();
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="relative max-w-[90vw] max-h-[90vh] flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Toolbar */}
        <div className="flex items-center justify-between gap-2 mb-2 px-1">
          <span className="text-xs text-white/70 truncate max-w-[60%]">{img.filename}</span>
          <div className="flex items-center gap-1.5">
            {img.width && img.height && (
              <span className="text-[10px] text-white/50">
                {img.width} × {img.height}
              </span>
            )}
            <button
              onClick={handleDownload}
              className="p-1.5 rounded hover:bg-white/10 text-white/70 hover:text-white transition-colors"
              title="Download"
            >
              <Download className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-white/10 text-white/70 hover:text-white transition-colors"
              title="Close"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Media */}
        {isVideo(img) ? (
          <video
            src={currentSrc}
            controls
            loop
            autoPlay
            className="max-w-[90vw] max-h-[75vh] object-contain rounded-md border border-white/10"
          />
        ) : (
          <img
            src={currentSrc}
            alt={img.prompt ?? img.filename}
            className="max-w-[90vw] max-h-[75vh] object-contain rounded-md border border-white/10"
          />
        )}

        {/* Prompt */}
        {img.prompt && (
          <p className="mt-2 text-xs text-white/60 italic line-clamp-2 text-center px-4">
            "{img.prompt}"
          </p>
        )}

        {/* Navigation arrows */}
        {idx > 0 && (
          <button
            onClick={prev}
            className="absolute left-[-44px] top-1/2 -translate-y-1/2 p-2 rounded-full bg-black/50 hover:bg-black/80 text-white transition-colors"
          >
            <ChevronLeft className="w-5 h-5" />
          </button>
        )}
        {idx < images.length - 1 && (
          <button
            onClick={next}
            className="absolute right-[-44px] top-1/2 -translate-y-1/2 p-2 rounded-full bg-black/50 hover:bg-black/80 text-white transition-colors"
          >
            <ChevronRight className="w-5 h-5" />
          </button>
        )}
      </div>
    </div>
  );
}

// ── Image tile ────────────────────────────────────────────────────────────────

const ImageTile = memo(function ImageTile({
  image,
  onClick,
  onDelete,
}: {
  image: GalleryImage;
  onClick: () => void;
  onDelete: () => void;
}) {
  const [deleting, setDeleting] = useState(false);

  // Memoized by the stable identity fields so the browser never sees a
  // spurious src change when the parent re-renders with a new array reference.
  const src = useMemo(() => mediaSrc(image), [image.id, image.file_path, image.image_data]);

  const handleDelete = async (e: React.MouseEvent) => {
    e.stopPropagation();
    setDeleting(true);
    onDelete();
  };

  const handleDownload = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isVideo(image)) {
      window.open(src, "_blank");
    } else {
      const a = document.createElement("a");
      a.href = src;
      a.download = image.filename;
      a.click();
    }
  };

  return (
    <div
      className={cn(
        "group relative rounded-md overflow-hidden border border-border bg-secondary cursor-pointer",
        "transition-all hover:border-primary/50 hover:shadow-md hover:shadow-primary/10",
        deleting && "opacity-50 pointer-events-none"
      )}
      onClick={onClick}
      title={image.prompt ?? image.filename}
    >
      <div className="aspect-square">
        {isVideo(image) ? (
          <video
            src={src}
            muted
            loop
            className="w-full h-full object-cover"
            onMouseEnter={(e) => e.currentTarget.play()}
            onMouseLeave={(e) => { e.currentTarget.pause(); e.currentTarget.currentTime = 0; }}
          />
        ) : (
          <img
            src={src}
            alt={image.prompt ?? image.filename}
            className="w-full h-full object-cover"
            loading="lazy"
          />
        )}
      </div>

      {/* Hover overlay */}
      <div className="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 transition-opacity flex flex-col justify-between p-1.5">
        <div className="flex justify-end gap-1">
          <button
            onClick={handleDownload}
            className="p-1 rounded bg-black/60 hover:bg-black/80 text-white transition-colors"
            title="Download"
          >
            <Download className="w-3 h-3" />
          </button>
          <button
            onClick={handleDelete}
            className="p-1 rounded bg-red-500/70 hover:bg-red-500 text-white transition-colors"
            title="Delete"
          >
            <Trash2 className="w-3 h-3" />
          </button>
        </div>
        <p className="text-[9px] text-white/80 line-clamp-1 font-mono leading-tight">
          {formatDate(image.created_at)}
        </p>
      </div>
    </div>
  );
});

// ── Upload area ───────────────────────────────────────────────────────────────

function UploadArea({ conversationId }: { conversationId: string | null }) {
  const { saveUpload } = useGalleryStore();
  const fileRef = useRef<HTMLInputElement>(null);
  const [uploading, setUploading] = useState(false);

  const handleFiles = async (files: FileList | null) => {
    if (!files || !conversationId) return;
    setUploading(true);
    for (const file of Array.from(files)) {
      if (!file.type.startsWith("image/")) continue;
      const reader = new FileReader();
      await new Promise<void>((resolve) => {
        reader.onload = async () => {
          const dataUrl = reader.result as string;
          // Strip "data:image/...;base64," prefix
          const base64 = dataUrl.split(",")[1];
          await saveUpload(conversationId, file.name, base64, file.type);
          resolve();
        };
        reader.readAsDataURL(file);
      });
    }
    setUploading(false);
  };

  return (
    <div className="p-3">
      <div
        className={cn(
          "border-2 border-dashed border-border rounded-lg p-6 text-center",
          "hover:border-primary/50 transition-colors cursor-pointer",
          "flex flex-col items-center gap-2"
        )}
        onClick={() => fileRef.current?.click()}
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          e.preventDefault();
          handleFiles(e.dataTransfer.files);
        }}
      >
        {uploading ? (
          <Loader2 className="w-6 h-6 text-muted-foreground animate-spin" />
        ) : (
          <Upload className="w-6 h-6 text-muted-foreground" />
        )}
        <p className="text-xs text-muted-foreground">
          {uploading ? "Uploading…" : "Click or drag images here"}
        </p>
        {!conversationId && (
          <p className="text-[10px] text-amber-400">Select a conversation first</p>
        )}
      </div>
      <input
        ref={fileRef}
        type="file"
        accept="image/*"
        multiple
        className="hidden"
        onChange={(e) => handleFiles(e.target.files)}
      />
    </div>
  );
}

// ── Empty state ───────────────────────────────────────────────────────────────

function EmptyState({ label }: { label: string }) {
  return (
    <div className="flex flex-col items-center justify-center h-32 gap-2 text-muted-foreground">
      <ImageOff className="w-8 h-8 opacity-30" />
      <p className="text-xs">{label}</p>
    </div>
  );
}

// ── Main panel ────────────────────────────────────────────────────────────────

export function GalleryPanel() {
  const {
    images,
    scope,
    activeConversationId,
    isInitialized,
    closeGallery,
    deleteImage,
    fetchImages,
    fetchAllImages,
    setScope,
  } = useGalleryStore();

  const [tab, setTab] = useState<"generated" | "uploads">("generated");
  const [lightboxIdx, setLightboxIdx] = useState<number | null>(null);

  // Load images when panel opens or scope changes
  useEffect(() => {
    if (scope === "all") {
      fetchAllImages();
    } else if (activeConversationId) {
      fetchImages(activeConversationId);
    }
  }, [scope, activeConversationId, fetchImages, fetchAllImages]);

  const filtered = images.filter((img) => img.source === tab);

  // Lightbox needs to index into the filtered list
  const openLightbox = (idx: number) => setLightboxIdx(idx);

  return (
    <>
      <div className="flex flex-col h-full bg-background">
        {/* Header */}
        <div className="flex items-center justify-between px-3 py-2 border-b border-border shrink-0">
          <div className="flex items-center gap-2">
            <Images className="w-4 h-4 text-muted-foreground" />
            <span className="text-sm font-medium">Gallery</span>
          </div>

          <div className="flex items-center gap-1">
            {/* Scope toggle */}
            <div className="flex rounded-md border border-border overflow-hidden text-[10px]">
              <button
                className={cn(
                  "px-2 py-1 transition-colors",
                  scope === "conversation"
                    ? "glass-primary text-white"
                    : "text-muted-foreground hover:bg-secondary"
                )}
                onClick={() => setScope("conversation")}
              >
                This Chat
              </button>
              <button
                className={cn(
                  "px-2 py-1 transition-colors",
                  scope === "all"
                    ? "glass-primary text-white"
                    : "text-muted-foreground hover:bg-secondary"
                )}
                onClick={() => setScope("all")}
              >
                All
              </button>
            </div>

            <button
              onClick={closeGallery}
              className="p-1 rounded hover:bg-secondary text-muted-foreground hover:text-foreground transition-colors"
              title="Close gallery"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Tabs */}
        <div className="flex border-b border-border shrink-0">
          {(["generated", "uploads"] as const).map((t) => (
            <button
              key={t}
              className={cn(
                "flex-1 py-1.5 text-xs font-medium capitalize transition-colors",
                tab === t
                  ? "border-b-2 border-primary text-foreground"
                  : "text-muted-foreground hover:text-foreground"
              )}
              onClick={() => setTab(t)}
            >
              {t}
              {images.filter((img) => img.source === t).length > 0 && (
                <span className="ml-1 text-[9px] bg-secondary rounded-full px-1.5 py-0.5">
                  {images.filter((img) => img.source === t).length}
                </span>
              )}
            </button>
          ))}
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto">
          {tab === "uploads" && (
            <UploadArea conversationId={activeConversationId} />
          )}

          {!isInitialized ? (
            <div className="flex items-center justify-center h-32">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
            </div>
          ) : filtered.length === 0 ? (
            <EmptyState
              label={
                tab === "generated"
                  ? "No generated images yet"
                  : "No uploaded images yet"
              }
            />
          ) : (
            <div className="p-3 grid grid-cols-3 gap-2">
              {filtered.map((img, i) => (
                <ImageTile
                  key={img.id}
                  image={img}
                  onClick={() => openLightbox(i)}
                  onDelete={() => deleteImage(img.id)}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Lightbox */}
      {lightboxIdx !== null && filtered.length > 0 && (
        <Lightbox
          images={filtered}
          startIndex={lightboxIdx}
          onClose={() => setLightboxIdx(null)}
        />
      )}
    </>
  );
}
