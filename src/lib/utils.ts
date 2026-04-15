import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { GalleryImage } from "./tauri";

/**
 * Converts a byte array to a base64 string safely for any file size.
 * The naive spread approach (`String.fromCharCode(...bytes)`) hits the JS
 * call-stack argument limit on large files and throws a RangeError.
 */
export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const CHUNK = 8192;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/** Returns the MIME type for a common image extension, defaulting to image/jpeg. */
export function imageMime(ext: string): string {
  switch (ext.toLowerCase()) {
    case "png": return "image/png";
    case "gif": return "image/gif";
    case "webp": return "image/webp";
    case "bmp": return "image/bmp";
    default: return "image/jpeg";
  }
}

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export function formatDate(dateStr: string): string {
  try {
    return new Date(dateStr).toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return dateStr;
  }
}

export function truncate(str: string, maxLength: number): string {
  if (str.length <= maxLength) return str;
  return str.slice(0, maxLength - 3) + "...";
}

export function debounce<T extends (...args: unknown[]) => unknown>(
  fn: T,
  delay: number
): (...args: Parameters<T>) => void {
  let timer: ReturnType<typeof setTimeout>;
  return (...args: Parameters<T>) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), delay);
  };
}

/**
 * Resolve the display URL for a gallery image.
 * Priority: on-disk file (via asset:// protocol) > base64 data URL > raw http URL.
 */
export function resolveGallerySrc(img: GalleryImage): string {
  if (img.file_path) {
    try {
      return convertFileSrc(img.file_path);
    } catch {
      // convertFileSrc unavailable outside Tauri — fall through
    }
  }
  const d = img.image_data;
  if (d && (d.startsWith("http://") || d.startsWith("https://"))) return d;
  if (d && d.length > 0) return `data:${img.mime_type};base64,${d}`;
  return "";
}
