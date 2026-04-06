import { Artifact } from "./types";

export function formatDate(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const diff = now.getTime() - d.getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "Just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return d.toLocaleDateString();
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

export function base64ToDataUri(data: string, mime: string): string {
  return `data:${mime};base64,${data}`;
}

export function artifactLanguage(art: Artifact): string {
  switch (art.artifact_type) {
    case "code": return art.language || "text";
    case "html": return "html";
    case "markdown": return "markdown";
    case "csv": return "csv";
    case "json": return "json";
    default: return "text";
  }
}

export function artifactIcon(type: Artifact["artifact_type"]): string {
  switch (type) {
    case "code": return "code-slash";
    case "html": return "globe-outline";
    case "markdown": return "document-text-outline";
    case "csv": return "grid-outline";
    case "json": return "construct-outline";
    default: return "document-outline";
  }
}

export function truncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen) + "…";
}

export function stripThinking(text: string): string {
  return text
    .replace(/<think>[\s\S]*?<\/think>/gi, "")
    .replace(/^<｜thinking▎\w+｜>[\s\S]*?<\/thinking>/gim, "")
    .trim();
}
