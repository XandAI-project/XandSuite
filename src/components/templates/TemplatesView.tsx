import { useEffect, useMemo, useState } from "react";
import {
  LayoutTemplate,
  Plus,
  Pencil,
  Trash2,
  Loader2,
  Hash,
  TrendingUp,
  Search,
  Package,
} from "lucide-react";
import { useTemplateStore } from "@/stores/templateStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { PromptTemplate } from "@/lib/tauri";
import { TemplateEditor } from "./TemplateEditor";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

// ── Category colours ─────────────────────────────────────────────────────────

const CATEGORY_COLOURS: Record<string, string> = {
  Writing:     "bg-blue-500/15 text-blue-300 border-blue-500/30",
  Code:        "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  Research:    "bg-violet-500/15 text-violet-300 border-violet-500/30",
  Analysis:    "bg-amber-500/15 text-amber-300 border-amber-500/30",
  Creative:    "bg-pink-500/15 text-pink-300 border-pink-500/30",
  Media:       "bg-cyan-500/15 text-cyan-300 border-cyan-500/30",
  Productivity:"bg-teal-500/15 text-teal-300 border-teal-500/30",
};
const DEFAULT_COLOUR = "bg-secondary text-muted-foreground border-border";

function catColour(cat?: string) {
  return cat ? (CATEGORY_COLOURS[cat] ?? DEFAULT_COLOUR) : DEFAULT_COLOUR;
}

// Highlight {{variable}} tokens in a string
function HighlightedContent({ text }: { text: string }) {
  const parts = text.split(/(\{\{\w+\}\})/g);
  return (
    <>
      {parts.map((part, i) =>
        /^\{\{\w+\}\}$/.test(part) ? (
          <span
            key={i}
            className="bg-amber-500/15 text-amber-300 rounded px-0.5 font-mono text-[11px]"
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

// ── Template card ─────────────────────────────────────────────────────────────

function TemplateCard({
  template,
  onEdit,
  onDelete,
}: {
  template: PromptTemplate;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [deleting, setDeleting] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const { deleteTemplate } = useTemplateStore();

  const handleDelete = async () => {
    setDeleting(true);
    try {
      await deleteTemplate(template.id);
      onDelete();
    } finally {
      setDeleting(false);
    }
  };

  const preview = template.content.slice(0, 120) + (template.content.length > 120 ? "…" : "");

  return (
    <div className="group flex flex-col gap-3 rounded-xl border border-border bg-card p-5 transition-all hover:border-primary/40 hover:shadow-md hover:shadow-primary/5">
      {/* Header row */}
      <div className="flex items-start gap-3">
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10">
          <LayoutTemplate className="w-4 h-4 text-primary" />
        </div>
        <div className="flex-1 min-w-0">
          <p className="font-semibold truncate text-sm">{template.title}</p>
          {template.description && (
            <p className="mt-0.5 text-xs text-muted-foreground truncate">{template.description}</p>
          )}
        </div>
      </div>

      {/* Content preview */}
      <p className="text-xs text-muted-foreground/80 font-mono bg-secondary/40 rounded-lg px-3 py-2 line-clamp-3 leading-relaxed whitespace-pre-wrap">
        <HighlightedContent text={preview} />
      </p>

      {/* Chips row */}
      <div className="flex items-center gap-2 flex-wrap">
        {template.category && (
          <span className={cn(
            "text-[10px] px-2 py-0.5 rounded-full border",
            catColour(template.category)
          )}>
            {template.category}
          </span>
        )}
        {template.shortcut && (
          <span className="flex items-center gap-0.5 text-[10px] px-1.5 py-0.5 rounded bg-primary/10 text-primary font-mono">
            <Hash className="w-2.5 h-2.5" />
            {template.shortcut}
          </span>
        )}
        {template.requires && (
          <span className="flex items-center gap-1 text-[10px] px-2 py-0.5 rounded-full border bg-orange-500/10 text-orange-300 border-orange-500/30">
            <Package className="w-2.5 h-2.5 shrink-0" />
            Requires: {template.requires}
          </span>
        )}
        {template.use_count > 0 && (
          <span className="flex items-center gap-0.5 text-[10px] text-muted-foreground/60 ml-auto">
            <TrendingUp className="w-2.5 h-2.5" />
            {template.use_count} use{template.use_count !== 1 ? "s" : ""}
          </span>
        )}
      </div>

      {/* Action buttons */}
      <div className="flex items-center gap-2 pt-1">
        <Button
          size="sm"
          variant="outline"
          className="flex-1 gap-1.5"
          onClick={onEdit}
        >
          <Pencil className="w-3.5 h-3.5" />
          Edit
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="text-destructive hover:text-destructive hover:bg-destructive/10"
          onClick={() => setConfirmOpen(true)}
          disabled={deleting}
        >
          {deleting ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
          ) : (
            <Trash2 className="w-3.5 h-3.5" />
          )}
        </Button>
      </div>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{template.title}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This template will be permanently deleted and removed from the slash-command list.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleDelete}>Delete</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

// ── Main view ─────────────────────────────────────────────────────────────────

export function TemplatesView() {
  const { templates, loading, fetchTemplates } = useTemplateStore();
  const [editorOpen, setEditorOpen] = useState(false);
  const [editingTemplate, setEditingTemplate] = useState<PromptTemplate | null>(null);
  const [search, setSearch] = useState("");
  const [activeCategory, setActiveCategory] = useState<string | null>(null);

  useEffect(() => {
    fetchTemplates();
  }, []);  // eslint-disable-line react-hooks/exhaustive-deps

  // Derive unique category list
  const categories = useMemo(() => {
    const cats = new Set<string>();
    templates.forEach((t) => { if (t.category) cats.add(t.category); });
    return Array.from(cats).sort();
  }, [templates]);

  const filtered = useMemo(() => {
    let list = templates;
    if (activeCategory) list = list.filter((t) => t.category === activeCategory);
    if (search.trim()) {
      const q = search.toLowerCase();
      list = list.filter(
        (t) =>
          t.title.toLowerCase().includes(q) ||
          t.content.toLowerCase().includes(q) ||
          (t.description ?? "").toLowerCase().includes(q) ||
          (t.shortcut ?? "").toLowerCase().includes(q)
      );
    }
    return list;
  }, [templates, activeCategory, search]);

  const handleEdit = (template: PromptTemplate) => {
    setEditingTemplate(template);
    setEditorOpen(true);
  };

  const handleNew = () => {
    setEditingTemplate(null);
    setEditorOpen(true);
  };

  return (
    <div className="flex h-full flex-col bg-background">
      {/* Page header */}
      <div className="flex items-center justify-between border-b border-border px-6 py-4 shrink-0">
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-primary/10">
            <LayoutTemplate className="w-5 h-5 text-primary" />
          </div>
          <div>
            <h1 className="text-lg font-semibold">Prompt Templates</h1>
            <p className="text-xs text-muted-foreground">
              {templates.length} template{templates.length !== 1 ? "s" : ""} · type <kbd className="rounded px-1 py-0.5 bg-secondary text-[10px] font-mono">/</kbd> in chat to quick-insert
            </p>
          </div>
        </div>
        <Button onClick={handleNew} className="gap-2">
          <Plus className="w-4 h-4" />
          New Template
        </Button>
      </div>

      {/* Filter bar */}
      <div className="flex items-center gap-3 border-b border-border px-6 py-3 shrink-0 flex-wrap">
        <div className="relative flex-1 min-w-48">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
          <Input
            className="pl-8 h-8 text-sm"
            placeholder="Search templates…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        {/* Category filter pills */}
        <div className="flex items-center gap-1.5 flex-wrap">
          <button
            onClick={() => setActiveCategory(null)}
            className={cn(
              "text-xs px-3 py-1 rounded-full border transition-colors",
              activeCategory === null
                ? "glass-primary text-white border-blue-400/40"
                : "text-muted-foreground border-border hover:border-primary/50 hover:text-foreground"
            )}
          >
            All
          </button>
          {categories.map((cat) => (
            <button
              key={cat}
              onClick={() => setActiveCategory(activeCategory === cat ? null : cat)}
              className={cn(
                "text-xs px-3 py-1 rounded-full border transition-colors",
                activeCategory === cat
                  ? "glass-primary text-white border-blue-400/40"
                  : cn("border", catColour(cat), "hover:opacity-80")
              )}
            >
              {cat}
            </button>
          ))}
        </div>
      </div>

      {/* Grid */}
      <div className="flex-1 overflow-y-auto p-6">
        {loading ? (
          <div className="flex items-center justify-center h-32 text-muted-foreground gap-2">
            <Loader2 className="w-4 h-4 animate-spin" />
            Loading templates…
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-48 gap-3 text-center">
            <LayoutTemplate className="w-10 h-10 text-muted-foreground/30" />
            <div>
              <p className="text-sm font-medium text-muted-foreground">
                {search || activeCategory ? "No templates match your filter" : "No templates yet"}
              </p>
              {!search && !activeCategory && (
                <p className="mt-1 text-xs text-muted-foreground/60">
                  Click "New Template" to create your first one.
                </p>
              )}
            </div>
          </div>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {filtered.map((template) => (
              <TemplateCard
                key={template.id}
                template={template}
                onEdit={() => handleEdit(template)}
                onDelete={() => {}}
              />
            ))}
          </div>
        )}
      </div>

      {/* Editor modal */}
      <TemplateEditor
        open={editorOpen}
        template={editingTemplate}
        onClose={() => { setEditorOpen(false); setEditingTemplate(null); }}
      />
    </div>
  );
}
