import { useEffect, useRef, useState } from "react";
import { X, LayoutTemplate, Hash, Package } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useTemplateStore } from "@/stores/templateStore";
import { cn } from "@/lib/utils";
import type { PromptTemplate } from "@/lib/tauri";

// Highlight {{variable}} tokens with a warm amber style
function HighlightedContent({ text }: { text: string }) {
  const parts = text.split(/(\{\{\w+\}\})/g);
  return (
    <>
      {parts.map((part, i) =>
        /^\{\{\w+\}\}$/.test(part) ? (
          <span
            key={i}
            className="bg-amber-500/15 text-amber-300 rounded px-0.5 font-mono text-xs"
          >
            {part}
          </span>
        ) : (
          <span key={i}>{part}</span>
        )
      )}
    </>
  );
}

// Existing category suggestions (rendered as datalist options)
const BUILTIN_CATEGORIES = ["Writing", "Code", "Research", "Analysis", "Productivity", "Other"];

interface Props {
  open: boolean;
  template: PromptTemplate | null;
  onClose: () => void;
}

export function TemplateEditor({ open, template, onClose }: Props) {
  const isEditing = template !== null;
  const { createTemplate, updateTemplate } = useTemplateStore();

  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  const [description, setDescription] = useState("");
  const [category, setCategory] = useState("");
  const [shortcut, setShortcut] = useState("");
  const [requires, setRequires] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const titleRef = useRef<HTMLInputElement>(null);

  // Seed form when opening
  useEffect(() => {
    if (!open) return;
    if (template) {
      setTitle(template.title);
      setContent(template.content);
      setDescription(template.description ?? "");
      setCategory(template.category ?? "");
      setShortcut(template.shortcut ?? "");
      setRequires(template.requires ?? "");
    } else {
      setTitle("");
      setContent("");
      setDescription("");
      setCategory("");
      setShortcut("");
      setRequires("");
    }
    setError(null);
    setTimeout(() => titleRef.current?.focus(), 60);
  }, [open, template?.id]);  // eslint-disable-line react-hooks/exhaustive-deps

  const handleSave = async () => {
    if (!title.trim()) { setError("Title is required."); return; }
    if (!content.trim()) { setError("Content is required."); return; }

    setSaving(true);
    setError(null);
    try {
      const shortcutVal = shortcut.trim()
        ? (shortcut.startsWith("/") ? shortcut.trim() : `/${shortcut.trim()}`)
        : undefined;

      if (isEditing && template) {
        await updateTemplate({
          id: template.id,
          title: title.trim(),
          content,
          description: description.trim() || undefined,
          category: category.trim() || undefined,
          shortcut: shortcutVal,
          requires: requires.trim() || undefined,
        });
      } else {
        await createTemplate({
          title: title.trim(),
          content,
          description: description.trim() || undefined,
          category: category.trim() || undefined,
          shortcut: shortcutVal,
          requires: requires.trim() || undefined,
        });
      }
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onClose();
  };

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onKeyDown={handleKeyDown}
    >
      <div className="relative flex h-[90vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-border bg-background shadow-2xl">
        {/* Header */}
        <div className="flex items-center gap-3 border-b border-border px-6 py-4 shrink-0">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary/10">
            <LayoutTemplate className="w-4 h-4 text-primary" />
          </div>
          <h2 className="flex-1 font-semibold">
            {isEditing ? "Edit Template" : "New Template"}
          </h2>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Scrollable body */}
        <ScrollArea className="flex-1">
          <div className="flex flex-col gap-5 p-6">
            {/* Title */}
            <div>
              <label className="mb-1.5 block text-sm font-medium">Title <span className="text-destructive">*</span></label>
              <Input
                ref={titleRef}
                placeholder="e.g. Summarise text"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
              />
            </div>

            {/* Category + Shortcut row */}
            <div className="flex gap-4">
              <div className="flex-1">
                <label className="mb-1.5 block text-sm font-medium">Category</label>
                <Input
                  placeholder="e.g. Writing"
                  value={category}
                  onChange={(e) => setCategory(e.target.value)}
                  list="template-categories"
                />
                <datalist id="template-categories">
                  {BUILTIN_CATEGORIES.map((c) => <option key={c} value={c} />)}
                </datalist>
              </div>
              <div className="flex-1">
                <label className="mb-1.5 block text-sm font-medium">
                  Shortcut
                  <span className="ml-1.5 text-xs text-muted-foreground">(e.g. /sum)</span>
                </label>
                <div className="relative">
                  <Hash className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
                  <Input
                    className="pl-8"
                    placeholder="summarise"
                    value={shortcut.replace(/^\//, "")}
                    onChange={(e) => setShortcut(e.target.value.replace(/^\//, ""))}
                  />
                </div>
                {shortcut && (
                  <p className="mt-1 text-[11px] text-muted-foreground font-mono">
                    Will trigger as: <span className="text-primary">/{shortcut.replace(/^\//, "")}</span>
                  </p>
                )}
              </div>
            </div>

            {/* Description + Requires row */}
            <div className="flex gap-4">
              <div className="flex-1">
                <label className="mb-1.5 block text-sm font-medium">Description</label>
                <Input
                  placeholder="Short description of what this template does"
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                />
              </div>
              <div className="flex-[0_0_200px]">
                <label className="mb-1.5 block text-sm font-medium">
                  Requires package
                  <span className="ml-1.5 text-xs text-muted-foreground">(optional)</span>
                </label>
                <div className="relative">
                  <Package className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
                  <Input
                    className="pl-8"
                    placeholder="e.g. Rich Responses"
                    value={requires}
                    onChange={(e) => setRequires(e.target.value)}
                  />
                </div>
              </div>
            </div>

            {/* Content */}
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <label className="text-sm font-medium">Content <span className="text-destructive">*</span></label>
                <span className="text-[11px] text-muted-foreground">
                  Use <code className="bg-amber-500/10 text-amber-300 px-1 rounded">{"{{variableName}}"}</code> for placeholders
                </span>
              </div>
              <textarea
                className={cn(
                  "w-full resize-y rounded-lg border border-border bg-secondary/30 px-3 py-2.5",
                  "text-sm font-mono placeholder:text-muted-foreground outline-none",
                  "focus:border-primary/60 transition-colors min-h-[160px]"
                )}
                placeholder={"Summarise the following text:\n\n{{text}}"}
                value={content}
                onChange={(e) => setContent(e.target.value)}
                rows={8}
              />
            </div>

            {/* Live preview */}
            {content && (
              <div className="rounded-lg border border-border bg-secondary/30 p-4">
                <p className="mb-2 text-xs font-medium text-muted-foreground">Preview</p>
                <p className="text-sm whitespace-pre-wrap leading-relaxed text-foreground/80">
                  <HighlightedContent text={content} />
                </p>
              </div>
            )}

            {error && (
              <p className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-2 text-sm text-destructive">
                {error}
              </p>
            )}
          </div>
        </ScrollArea>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 border-t border-border px-6 py-4 shrink-0">
          <Button variant="outline" onClick={onClose} disabled={saving}>
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={saving}>
            {saving ? "Saving…" : isEditing ? "Save changes" : "Create template"}
          </Button>
        </div>
      </div>
    </div>
  );
}
