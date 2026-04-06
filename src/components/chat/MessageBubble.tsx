import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import {
  User, Bot, Copy, Check, ChevronDown, ChevronRight, Brain,
  Gauge, Pencil, RefreshCw, X,
  FileText, FileCode, FileJson, File,
  Code, Globe, AlignLeft, Terminal, BookOpen,
} from "lucide-react";
import { useState, useRef, useEffect } from "react";
import { cn } from "@/lib/utils";
import type { Message, ArtifactType, AttachmentMeta, ImageMeta } from "@/lib/tauri";
import { ArtifactCard } from "./ArtifactCard";
import { useArtifactStore } from "@/stores/artifactStore";
import { useSkillsStore } from "@/stores/skillsStore";

// ── SourcesCard ────────────────────────────────────────────────────────────────

interface RagSource {
  content: string;
  source: string;
  score: number;
  entities?: string[];
}

function SourcesCard({ sources }: { sources: RagSource[] }) {
  const [open, setOpen] = useState(false);
  if (!sources || sources.length === 0) return null;
  return (
    <div className="mt-2 rounded-lg border border-border/60 bg-card/40 text-xs overflow-hidden">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-2 w-full px-3 py-2 text-left hover:bg-secondary/50 transition-colors"
      >
        <BookOpen className="w-3 h-3 text-primary/70 shrink-0" />
        <span className="font-medium text-muted-foreground">
          {sources.length} source{sources.length > 1 ? "s" : ""} retrieved
        </span>
        <ChevronRight className={cn("w-3 h-3 ml-auto text-muted-foreground transition-transform", open && "rotate-90")} />
      </button>
      {open && (
        <div className="border-t border-border/60 divide-y divide-border/40">
          {sources.map((s, i) => (
            <div key={i} className="px-3 py-2 space-y-1">
              <div className="flex items-center justify-between gap-2">
                <span className="font-medium text-muted-foreground truncate">{s.source || "document"}</span>
                <span className="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-primary/10 text-primary/80 font-medium">
                  {(s.score * 100).toFixed(0)}%
                </span>
              </div>
              <p className="text-muted-foreground/80 leading-relaxed line-clamp-2">{s.content}</p>
              {s.entities && s.entities.length > 0 && (
                <div className="flex flex-wrap gap-1 mt-1">
                  {s.entities.slice(0, 4).map((e, ei) => (
                    <span key={ei} className="text-[9px] px-1 py-0.5 rounded bg-purple-500/15 text-purple-300 border border-purple-500/20">
                      {e}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function attachmentIcon(filename: string) {
  const ext = filename.split(".").pop()?.toLowerCase() ?? "";
  if (["csv", "tsv", "txt", "md", "pdf"].includes(ext)) return <FileText className="w-3 h-3" />;
  if (["json", "jsonl"].includes(ext)) return <FileJson className="w-3 h-3" />;
  if (["js", "ts", "py", "rs", "go", "java", "cpp", "c", "cs", "html", "xml"].includes(ext))
    return <FileCode className="w-3 h-3" />;
  return <File className="w-3 h-3" />;
}

function parseAttachmentMeta(metadata?: AttachmentMeta | null): AttachmentMeta | null {
  if (!metadata) return null;
  const hasAttachments = Array.isArray(metadata.attachments) && metadata.attachments.length > 0;
  const hasImages = Array.isArray(metadata.images) && metadata.images.length > 0;
  if (!hasAttachments && !hasImages) return null;
  return metadata;
}

/** Renders a single image thumbnail from a persisted base64 ImageMeta object. */
function MessageImageThumb({ img }: { img: ImageMeta }) {
  const src = `data:${img.mime};base64,${img.data}`;
  return (
    <img
      src={src}
      alt={img.filename}
      title={img.filename}
      className="w-20 h-20 object-cover rounded-lg border border-primary-foreground/20 cursor-zoom-in"
      onClick={() => window.open(src, "_blank")}
    />
  );
}

// ── Streaming artifact detection ──────────────────────────────────────────────

interface StreamingArtifactInfo {
  textBefore: string;
  title: string;
  artifactType: ArtifactType;
  /** Characters streamed so far inside the artifact content. */
  contentChars: number;
}

/**
 * During streaming, detect an open <artifact> tag with no matching closing tag yet.
 * Returns the text before the tag, artifact metadata, and how many content chars
 * have been streamed so far so we can show a live size counter.
 */
function detectStreamingArtifact(raw: string): StreamingArtifactInfo | null {
  // If every open tag has a matching close tag, let normal parseArtifacts handle it
  const openCount = (raw.match(/<artifact[\s>]/gi) ?? []).length;
  const closeCount = (raw.match(/<\/artifact>/gi) ?? []).length;
  if (openCount === 0 || openCount === closeCount) return null;

  // Find the last unclosed opening tag (complete: includes closing >)
  const openTagRe = /<artifact\s+([^>]*)>/gi;
  let lastMatch: RegExpExecArray | null = null;
  let m: RegExpExecArray | null;
  while ((m = openTagRe.exec(raw)) !== null) lastMatch = m;

  // Partial tag: <artifact was seen but the > hasn't arrived yet — show a
  // generic badge immediately rather than waiting for the full opening tag.
  if (!lastMatch) {
    const partialIdx = raw.search(/<artifact/i);
    const textBefore = partialIdx > 0 ? raw.slice(0, partialIdx).trim() : "";
    const partialTypeMatch = raw.slice(partialIdx).match(/type="([^"]*)/i);
    const partialType = (partialTypeMatch?.[1] ?? "text") as ArtifactType;
    return { textBefore, title: "artifact", artifactType: partialType, contentChars: 0 };
  }

  const tagStart = lastMatch.index;
  const tagEnd = tagStart + lastMatch[0].length;
  const textBefore = raw.slice(0, tagStart).trim();
  const attrs: Record<string, string> = {};
  const attrRe = /(\w+)="([^"]*)"/g;
  let a: RegExpExecArray | null;
  while ((a = attrRe.exec(lastMatch[1])) !== null) attrs[a[1]] = a[2];

  // Count chars that have arrived after the opening tag
  const contentChars = raw.length - tagEnd;

  return {
    textBefore,
    title: attrs.title ?? "Artifact",
    artifactType: (attrs.type ?? "text") as ArtifactType,
    contentChars: Math.max(0, contentChars),
  };
}

// ── Animated ellipsis + Creating badge ───────────────────────────────────────

const TYPE_LABEL: Record<ArtifactType, string> = {
  code: "code",
  markdown: "document",
  html: "HTML",
  text: "text",
  csv: "CSV",
  json: "JSON",
};

const TYPE_ICON: Record<ArtifactType, React.ReactNode> = {
  code: <Code className="w-3.5 h-3.5" />,
  markdown: <FileText className="w-3.5 h-3.5" />,
  html: <Globe className="w-3.5 h-3.5" />,
  text: <AlignLeft className="w-3.5 h-3.5" />,
  csv: <FileText className="w-3.5 h-3.5" />,
  json: <FileJson className="w-3.5 h-3.5" />,
};

function AnimatedDots() {
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setFrame((f) => (f + 1) % 4), 500);
    return () => clearInterval(id);
  }, []);
  return <span className="inline-block w-5 text-left">{".".repeat(frame)}</span>;
}

function CreatingArtifactBadge({
  title,
  artifactType,
  contentChars,
}: {
  title: string;
  artifactType: ArtifactType;
  contentChars: number;
}) {
  const label = TYPE_LABEL[artifactType] ?? artifactType;
  const icon = TYPE_ICON[artifactType] ?? <File className="w-3.5 h-3.5" />;
  // Rough token estimate: ~4 chars per token (common approximation)
  const tokens = Math.round(contentChars / 4);
  const sizeLabel =
    contentChars === 0
      ? null
      : tokens >= 1000
      ? `~${(tokens / 1000).toFixed(1)}k tokens`
      : `~${tokens} tokens`;

  return (
    <div className="flex items-center gap-2 mt-2 px-3 py-2 rounded-lg border border-border bg-secondary/50 text-xs text-muted-foreground w-fit">
      <span className="text-primary/70">{icon}</span>
      <span>Creating {label}</span>
      {title && title !== "Artifact" && (
        <span className="text-foreground/60 font-medium max-w-[200px] truncate">"{title}"</span>
      )}
      {sizeLabel && (
        <span className="text-primary/60 font-mono tabular-nums">{sizeLabel}</span>
      )}
      <AnimatedDots />
    </div>
  );
}

function RunningCodeBadge({ language }: { language?: string }) {
  return (
    <div className="flex items-center gap-2 mt-2 px-3 py-2 rounded-lg border border-blue-500/30 bg-blue-500/10 text-xs text-blue-300 w-fit">
      <Terminal className="w-3.5 h-3.5 text-blue-400 shrink-0" />
      <span>Coding{language ? ` ${language}` : ""}</span>
      <AnimatedDots />
    </div>
  );
}

function PreparingResponseBadge() {
  return (
    <div className="flex items-center gap-2 mt-1 text-xs text-muted-foreground/60 w-fit">
      <span>Preparing response</span>
      <AnimatedDots />
    </div>
  );
}

interface Props {
  message: Message;
  liveThinking?: string;
  isThinking?: boolean;
  onEdit?: (messageId: string, newContent: string) => void;
  onRegenerate?: () => void;
  /** Avatar for the persona this conversation belongs to (shown instead of the Bot icon). */
  personaAvatar?: string;
  /** Persona name shown as tooltip on the avatar. */
  personaName?: string;
}

// ── Artifact parsing ──────────────────────────────────────────────────────────

interface ParsedArtifact {
  id: string;
  title: string;
  artifact_type: ArtifactType;
  language?: string;
  content: string;
}

function parseAttributes(attrString: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  const re = /(\w+)="([^"]*)"/g;
  let match;
  while ((match = re.exec(attrString)) !== null) {
    attrs[match[1]] = match[2];
  }
  return attrs;
}

/**
 * Strip markdown code fences that the LLM sometimes wraps inside artifact tags.
 * e.g. ```python\n...\n``` → ...
 */
function stripCodeFences(content: string): string {
  return content.replace(/^```[\w]*\n?([\s\S]*?)```\s*$/s, (_, inner) => inner).trim();
}

/**
 * Strip LLM-generated tool-call XML from visible content.
 * Models sometimes emit <execute_code>…</execute_code> or
 * <server__tool_name>…</server__tool_name> blocks in the content stream
 * alongside proper tool_calls structures. These XML blocks are internal
 * plumbing and must never be shown to the user.
 *
 * Detection heuristic: tag names that contain underscores are tool names,
 * not standard HTML elements.
 *
 * Also handles the streaming case where the closing tag hasn't arrived yet —
 * everything from an unclosed tool-call opening tag is hidden.
 */
function stripToolCallXml(content: string): string {
  // Pattern: <tag_with_underscores> ... </tag_with_underscores>
  const COMPLETE_RE = /<([a-z][a-z0-9]*(?:_[a-z0-9]+)+)>[\s\S]*?<\/\1>/gi;
  let result = content.replace(COMPLETE_RE, "");

  // During streaming the closing tag may not have arrived yet.
  // Hide everything from the first unclosed tool-call opening tag onward.
  const OPEN_RE = /<([a-z][a-z0-9]*(?:_[a-z0-9]+)+)>/i;
  const openMatch = OPEN_RE.exec(result);
  if (openMatch) {
    result = result.slice(0, openMatch.index);
  }

  return result.trim();
}

/**
 * Strip function-call planning syntax that some models write into their
 * thinking/reasoning blocks (e.g. Qwen-style):
 *   <function=tool_name>…</function>
 *   <parameter=param_name>…</parameter>
 * These are the model narrating its plan, not visible content.
 */
function stripReasoningToolCallXml(text: string): string {
  return text
    .replace(/<function=[^>]*>[\s\S]*?<\/function>/gi, "")
    .replace(/<parameter=[^>]*>[\s\S]*?<\/parameter>/gi, "")
    .replace(/<\/parameter>/gi, "")
    .replace(/<parameter=[^>]*>/gi, "")
    .replace(/<function=[^>]*>/gi, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

export function parseArtifacts(raw: string): {
  strippedContent: string;
  parsedArtifacts: ParsedArtifact[];
} {
  const parsedArtifacts: ParsedArtifact[] = [];
  const ARTIFACT_RE = /<artifact\s+([^>]*)>([\s\S]*?)<\/artifact>/gi;

  const strippedContent = raw.replace(ARTIFACT_RE, (_match, attrStr, content) => {
    const attrs = parseAttributes(attrStr);
    const artifactType = (attrs.type ?? "text") as ArtifactType;
    const cleanContent = stripCodeFences(content.trim());
    parsedArtifacts.push({
      id: `parsed-${parsedArtifacts.length}-${Date.now()}`,
      title: attrs.title ?? "Untitled",
      artifact_type: artifactType,
      language: attrs.language,
      content: cleanContent,
    });
    return "";
  });

  return { strippedContent: stripToolCallXml(strippedContent.trim()), parsedArtifacts };
}

// ── Code block with toolbar ───────────────────────────────────────────────────

function CodeBlock({ language, children }: { language: string; children: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(children);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="relative group/code rounded-lg overflow-hidden my-2 border border-border">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-[#1e1e1e] border-b border-border">
        <span className="text-[10px] text-muted-foreground font-mono">{language || "text"}</span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1 text-[10px] text-muted-foreground hover:text-foreground
                     transition-colors px-1.5 py-0.5 rounded hover:bg-white/10"
        >
          {copied ? (
            <><Check className="w-3 h-3 text-emerald-400" /><span className="text-emerald-400">Copied</span></>
          ) : (
            <><Copy className="w-3 h-3" />Copy</>
          )}
        </button>
      </div>
      <SyntaxHighlighter
        style={vscDarkPlus as Record<string, React.CSSProperties>}
        language={language || "text"}
        PreTag="div"
        className="!rounded-none !m-0 !text-xs"
        customStyle={{ borderRadius: 0, margin: 0 }}
      >
        {children}
      </SyntaxHighlighter>
    </div>
  );
}

// ── Markdown components shared between message + panel ────────────────────────

export const markdownComponents: React.ComponentProps<typeof ReactMarkdown>["components"] = {
  code({ className, children }) {
    const match = /language-(\w+)/.exec(className || "");
    const isBlock = String(children).includes("\n");
    return isBlock ? (
      <CodeBlock language={match ? match[1] : ""}>{String(children).replace(/\n$/, "")}</CodeBlock>
    ) : (
      <code className="px-1 py-0.5 rounded bg-muted text-xs font-mono">{children}</code>
    );
  },
  p: ({ children }) => <p className="mb-2 last:mb-0">{children}</p>,
  ul: ({ children }) => <ul className="list-disc pl-4 mb-2">{children}</ul>,
  ol: ({ children }) => <ol className="list-decimal pl-4 mb-2">{children}</ol>,
  li: ({ children }) => <li className="mb-0.5">{children}</li>,
  h1: ({ children }) => <h1 className="text-base font-bold mb-2 mt-3">{children}</h1>,
  h2: ({ children }) => <h2 className="text-sm font-bold mb-2 mt-3">{children}</h2>,
  h3: ({ children }) => <h3 className="text-sm font-semibold mb-1 mt-2">{children}</h3>,
  blockquote: ({ children }) => (
    <blockquote className="border-l-2 border-primary pl-3 italic text-muted-foreground my-2">
      {children}
    </blockquote>
  ),
  table: ({ children }) => (
    <div className="overflow-x-auto my-3">
      <table className="border-collapse w-full text-xs">{children}</table>
    </div>
  ),
  th: ({ children }) => (
    <th className="border border-border px-3 py-1.5 bg-secondary text-left font-semibold">{children}</th>
  ),
  td: ({ children }) => <td className="border border-border px-3 py-1.5">{children}</td>,
  a: ({ href, children }) => (
    <a href={href} target="_blank" rel="noreferrer" className="text-primary underline hover:no-underline">
      {children}
    </a>
  ),
  hr: () => <hr className="border-border my-3" />,
};

// ── Main component ────────────────────────────────────────────────────────────

export function MessageBubble({ message, liveThinking, isThinking, onEdit, onRegenerate, personaAvatar, personaName }: Props) {
  const [copied, setCopied] = useState(false);
  const [thinkOpen, setThinkOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editText, setEditText] = useState(message.content);
  const editRef = useRef<HTMLTextAreaElement>(null);
  const isUser = message.role === "user";

  const { artifacts } = useArtifactStore();

  useEffect(() => {
    if (editing && editRef.current) {
      editRef.current.focus();
      editRef.current.setSelectionRange(editRef.current.value.length, editRef.current.value.length);
    }
  }, [editing]);

  const confirmEdit = () => {
    const trimmed = editText.trim();
    if (trimmed && trimmed !== message.content) onEdit?.(message.id, trimmed);
    setEditing(false);
  };

  const cancelEdit = () => {
    setEditText(message.content);
    setEditing(false);
  };

  const thinkingText = stripReasoningToolCallXml(
    (message as Message & { thinking?: string }).thinking ?? liveThinking ?? ""
  );
  const tps = (message as Message & { tps?: number }).tps;

  const handleCopy = () => {
    navigator.clipboard.writeText(message.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  // Parse attachments from user message metadata
  const attachmentMeta = isUser ? parseAttachmentMeta(message.metadata) : null;

  const isStreamingMsg = message.id === "streaming";

  // During streaming, detect an in-progress artifact and show a badge instead of raw content
  const streamingArtifact = !isUser && isStreamingMsg
    ? detectStreamingArtifact(message.content)
    : null;

  // Parse artifacts from completed assistant messages
  const { strippedContent, parsedArtifacts } = !isUser && !streamingArtifact
    ? parseArtifacts(message.content)
    : {
        strippedContent: stripToolCallXml(streamingArtifact?.textBefore ?? message.content),
        parsedArtifacts: [],
      };

  // Match parsed artifacts to saved ones by title (for history messages)
  const resolvedArtifacts = parsedArtifacts.map((pa) => {
    const saved = artifacts.find(
      (a) => a.title === pa.title && a.artifact_type === pa.artifact_type
    );
    return saved ? { ...pa, id: saved.id } : pa;
  });

  // Detect a pending code-execution tool step so we can show "Coding python..."
  const activeToolSteps = useSkillsStore((s) => s.activeToolSteps);
  const pendingCodeStep = isStreamingMsg && !isUser
    ? activeToolSteps.find(
        (step) => step.function_name?.includes("execute_code") && step.result === undefined
      ) ?? null
    : null;
  const pendingCodeLanguage = pendingCodeStep?.arguments?.language as string | undefined;

  // Auto-open the reasoning section the first time thinking content appears
  const thinkingHasContent = !!thinkingText;
  useEffect(() => {
    if (thinkingHasContent) setThinkOpen(true);
  }, [thinkingHasContent]);

  return (
    <div className={cn("flex gap-3 group", isUser && "flex-row-reverse")}>
      {/* Avatar */}
      {isUser ? (
        <div className="flex-shrink-0 w-8 h-8 rounded-full flex items-center justify-center bg-primary">
          <User className="w-4 h-4 text-primary-foreground" />
        </div>
      ) : personaAvatar ? (
        personaAvatar.startsWith("data:") ? (
          <img
            src={personaAvatar}
            alt={personaName ?? "Persona"}
            title={personaName}
            className="flex-shrink-0 w-8 h-8 rounded-full object-cover ring-1 ring-border"
          />
        ) : (
          <div
            title={personaName}
            className="flex-shrink-0 w-8 h-8 rounded-full flex items-center justify-center bg-secondary text-base leading-none ring-1 ring-border"
          >
            {personaAvatar}
          </div>
        )
      ) : personaName ? (
        // Persona without an avatar — show initials
        <div
          title={personaName}
          className="flex-shrink-0 w-8 h-8 rounded-full flex items-center justify-center bg-primary/10 text-primary text-xs font-semibold ring-1 ring-border"
        >
          {personaName.split(" ").map((w) => w[0]).slice(0, 2).join("").toUpperCase()}
        </div>
      ) : (
        <div className="flex-shrink-0 w-8 h-8 rounded-full flex items-center justify-center bg-secondary">
          <Bot className="w-4 h-4 text-foreground" />
        </div>
      )}

      <div className={cn("flex flex-col max-w-[80%] gap-1.5", isUser && "items-end")}>

        {/* Thinking section */}
        {!isUser && (isThinking || thinkingText) && (
          <div className="rounded-lg border border-violet-500/30 bg-violet-500/5 text-xs w-full overflow-hidden">
            <button
              onClick={() => setThinkOpen((v) => !v)}
              className="flex items-center gap-1.5 w-full px-3 py-1.5 text-violet-300 hover:bg-violet-500/10 transition-colors"
            >
              <Brain className={cn("w-3 h-3 shrink-0", isThinking && "animate-pulse")} />
              <span className="font-medium">{isThinking ? "Thinking…" : "Reasoning"}</span>
              <span className="ml-auto text-violet-400/60 text-[10px]">
                {thinkingText.split(/\s+/).filter(Boolean).length} words
              </span>
              {thinkOpen ? <ChevronDown className="w-3 h-3 shrink-0" /> : <ChevronRight className="w-3 h-3 shrink-0" />}
            </button>
            {thinkOpen && (
              <div className="px-3 pb-2 pt-0.5 text-violet-200/70 whitespace-pre-wrap font-mono text-[11px] leading-relaxed border-t border-violet-500/20 max-h-64 overflow-y-auto">
                {thinkingText || <span className="animate-pulse">▌</span>}
              </div>
            )}
          </div>
        )}

        {/* Message bubble */}
        {isUser && editing ? (
          <div className="flex flex-col gap-2 w-full max-w-[80vw]">
            <textarea
              ref={editRef}
              value={editText}
              onChange={(e) => setEditText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); confirmEdit(); }
                if (e.key === "Escape") cancelEdit();
              }}
              rows={Math.min(10, editText.split("\n").length + 1)}
              className="w-full rounded-xl px-4 py-3 text-sm bg-primary/20 border border-primary/40
                         text-foreground resize-none focus:outline-none focus:ring-1 focus:ring-primary"
            />
            <div className="flex gap-2 justify-end">
              <button
                onClick={cancelEdit}
                className="flex items-center gap-1 text-xs px-3 py-1.5 rounded-lg border border-border
                           text-muted-foreground hover:bg-secondary transition-colors"
              >
                <X className="w-3 h-3" /> Cancel
              </button>
              <button
                onClick={confirmEdit}
                disabled={!editText.trim()}
                className="flex items-center gap-1 text-xs px-3 py-1.5 rounded-lg bg-primary
                           text-primary-foreground hover:bg-primary/90 disabled:opacity-50 transition-colors"
              >
                <RefreshCw className="w-3 h-3" /> Send
              </button>
            </div>
          </div>
        ) : (
          <div className={cn(
            "relative rounded-xl px-4 py-3 text-sm",
            isUser
              ? "bg-primary text-primary-foreground"
              : "bg-card border border-border text-foreground"
          )}>
            {isUser ? (
              <>
                {/* Image thumbnails (base64 stored in metadata) */}
                {attachmentMeta?.images && attachmentMeta.images.length > 0 && (
                  <div className="flex flex-wrap gap-2 mb-2 pb-2 border-b border-primary-foreground/20">
                    {attachmentMeta.images.map((img) => (
                      <MessageImageThumb key={img.filename} img={img} />
                    ))}
                  </div>
                )}
                {/* Text attachment chips */}
                {attachmentMeta?.attachments && attachmentMeta.attachments.length > 0 && (
                  <div className="flex flex-wrap gap-1 mb-2 pb-2 border-b border-primary-foreground/20">
                    {attachmentMeta.attachments.map((name) => (
                      <div
                        key={name}
                        className="flex items-center gap-1 px-1.5 py-0.5 rounded-full
                                   bg-primary-foreground/15 text-primary-foreground/80 text-[10px]"
                      >
                        <span className="opacity-70">{attachmentIcon(name)}</span>
                        <span className="max-w-[140px] truncate">{name}</span>
                      </div>
                    ))}
                  </div>
                )}
                <p className="whitespace-pre-wrap break-words">{message.content}</p>
              </>
            ) : (
              <>
                <div className="prose prose-invert prose-sm max-w-none">
                  {strippedContent ? (
                    <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
                      {strippedContent}
                    </ReactMarkdown>
                  ) : isStreamingMsg && !streamingArtifact && !pendingCodeStep && isThinking ? (
                    <PreparingResponseBadge />
                  ) : isStreamingMsg && !streamingArtifact && !pendingCodeStep ? (
                    <span className="animate-pulse text-muted-foreground">▌</span>
                  ) : !isStreamingMsg && thinkingText ? (
                    <span className="text-muted-foreground/60 text-xs italic">
                      (reasoning complete — no additional text response)
                    </span>
                  ) : !isStreamingMsg && resolvedArtifacts.length > 0 ? (
                    // Message has artifacts but no text body — valid, show nothing
                    null
                  ) : !isStreamingMsg ? (
                    <span className="animate-pulse text-muted-foreground">▌</span>
                  ) : null}
                </div>

                {/* In-progress artifact badge (shown while streaming) */}
                {streamingArtifact && (
                  <CreatingArtifactBadge
                    title={streamingArtifact.title}
                    artifactType={streamingArtifact.artifactType}
                    contentChars={streamingArtifact.contentChars}
                  />
                )}

                {/* Running code badge (shown while execute_code tool is in progress) */}
                {pendingCodeStep && !streamingArtifact && (
                  <RunningCodeBadge language={pendingCodeLanguage} />
                )}

                {/* Artifact cards (shown after streaming completes) */}
                {resolvedArtifacts.length > 0 && (
                  <div className="mt-1">
                    {resolvedArtifacts.map((a) => (
                      <ArtifactCard key={a.id} artifact={a} />
                    ))}
                  </div>
                )}

                {/* RAG source attribution */}
                {!isUser && (() => {
                  const meta = message.metadata as any;
                  const sources: RagSource[] | undefined = meta?.sources;
                  return sources && sources.length > 0
                    ? <SourcesCard sources={sources} />
                    : null;
                })()}
              </>
            )}

            {/* Copy button (assistant) */}
            {!isUser && message.content && (
              <button
                onClick={handleCopy}
                className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:bg-secondary"
              >
                {copied
                  ? <Check className="w-3 h-3 text-emerald-400" />
                  : <Copy className="w-3 h-3 text-muted-foreground" />}
              </button>
            )}
          </div>
        )}

        {/* Action row */}
        {!editing && (
          <div className={cn(
            "flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity",
            isUser ? "justify-end" : "justify-start pl-1"
          )}>
            {isUser && onEdit && (
              <button
                onClick={() => { setEditText(message.content); setEditing(true); }}
                className="flex items-center gap-1 text-[10px] text-muted-foreground hover:text-foreground
                           px-2 py-0.5 rounded hover:bg-secondary transition-colors"
              >
                <Pencil className="w-2.5 h-2.5" /> Edit
              </button>
            )}
            {!isUser && onRegenerate && message.content && (
              <button
                onClick={onRegenerate}
                className="flex items-center gap-1 text-[10px] text-muted-foreground hover:text-foreground
                           px-2 py-0.5 rounded hover:bg-secondary transition-colors"
              >
                <RefreshCw className="w-2.5 h-2.5" /> Regenerate
              </button>
            )}
            {!isUser && tps != null && (
              <span className="flex items-center gap-1 text-[10px] text-muted-foreground/50 ml-1">
                <Gauge className="w-2.5 h-2.5" /> {tps} tok/s
              </span>
            )}
          </div>
        )}

        {editing && !isUser && tps != null && (
          <div className="flex items-center gap-1 text-[10px] text-muted-foreground/50 pl-1">
            <Gauge className="w-2.5 h-2.5" /><span>{tps} tok/s</span>
          </div>
        )}
      </div>
    </div>
  );
}
