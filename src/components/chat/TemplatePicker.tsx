import { useEffect, useMemo, useRef, useState } from "react";
import { LayoutTemplate, Hash, TrendingUp } from "lucide-react";
import { useTemplateStore } from "@/stores/templateStore";
import { cn } from "@/lib/utils";
import type { PromptTemplate } from "@/lib/tauri";

// Category colour mapping — cycles through a palette
const CATEGORY_COLOURS: Record<string, string> = {
  Writing:  "bg-blue-500/15 text-blue-300 border-blue-500/30",
  Code:     "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  Research: "bg-violet-500/15 text-violet-300 border-violet-500/30",
  Analysis: "bg-amber-500/15 text-amber-300 border-amber-500/30",
};
const DEFAULT_CAT_COLOUR = "bg-secondary text-muted-foreground border-border";

function categoryColour(cat?: string) {
  if (!cat) return DEFAULT_CAT_COLOUR;
  return CATEGORY_COLOURS[cat] ?? DEFAULT_CAT_COLOUR;
}

interface Props {
  /** The current text typed after the "/" trigger, used to filter. */
  query: string;
  onSelect: (template: PromptTemplate) => void;
  onClose: () => void;
}

export function TemplatePicker({ query, onSelect, onClose }: Props) {
  const { templates } = useTemplateStore();
  const [cursor, setCursor] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  // Filter by query (matches title or shortcut)
  const filtered = useMemo(() => {
    const q = query.toLowerCase().replace(/^\//, "");
    if (!q) return templates;
    return templates.filter(
      (t) =>
        t.title.toLowerCase().includes(q) ||
        (t.shortcut ?? "").toLowerCase().includes(q) ||
        (t.category ?? "").toLowerCase().includes(q)
    );
  }, [templates, query]);

  // Reset cursor when filter changes
  useEffect(() => {
    setCursor(0);
  }, [query]);

  // Scroll active item into view
  useEffect(() => {
    const el = listRef.current?.querySelector(`[data-idx="${cursor}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  // Keyboard navigation — attached to the document so it intercepts from InputBar
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setCursor((c) => Math.min(c + 1, filtered.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setCursor((c) => Math.max(c - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (filtered[cursor]) onSelect(filtered[cursor]);
      } else if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [filtered, cursor, onSelect, onClose]);

  if (filtered.length === 0) {
    return (
      <div className="absolute bottom-full mb-2 left-0 right-0 rounded-xl border border-border bg-card shadow-xl p-4 text-sm text-muted-foreground text-center">
        No templates match "<span className="text-foreground">{query}</span>"
      </div>
    );
  }

  return (
    <div className="absolute bottom-full mb-2 left-0 right-0 rounded-xl border border-border bg-card shadow-xl overflow-hidden z-50">
      {/* Header */}
      <div className="flex items-center gap-2 px-3 py-2 border-b border-border bg-secondary/30">
        <LayoutTemplate className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
        <span className="text-xs text-muted-foreground">
          {filtered.length} template{filtered.length !== 1 ? "s" : ""}
          {query && <> matching <span className="text-foreground font-medium">{query}</span></>}
        </span>
        <span className="ml-auto text-[10px] text-muted-foreground/60">↑↓ navigate · Enter select · Esc close</span>
      </div>

      {/* List */}
      <div ref={listRef} className="max-h-64 overflow-y-auto">
        {filtered.map((template, idx) => (
          <button
            key={template.id}
            data-idx={idx}
            onClick={() => onSelect(template)}
            onMouseEnter={() => setCursor(idx)}
            className={cn(
              "w-full flex items-start gap-3 px-3 py-2.5 text-left transition-colors",
              idx === cursor ? "bg-primary/10" : "hover:bg-secondary/50"
            )}
          >
            {/* Icon */}
            <div className="flex-shrink-0 w-7 h-7 rounded-lg bg-secondary flex items-center justify-center mt-0.5">
              <LayoutTemplate className="w-3.5 h-3.5 text-muted-foreground" />
            </div>

            {/* Main content */}
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 flex-wrap">
                <span className="text-sm font-medium text-foreground">{template.title}</span>
                {template.shortcut && (
                  <span className="flex items-center gap-0.5 text-[10px] px-1.5 py-0.5 rounded bg-primary/10 text-primary font-mono">
                    <Hash className="w-2.5 h-2.5" />
                    {template.shortcut}
                  </span>
                )}
                {template.category && (
                  <span className={cn(
                    "text-[10px] px-1.5 py-0.5 rounded-full border",
                    categoryColour(template.category)
                  )}>
                    {template.category}
                  </span>
                )}
              </div>
              {template.description && (
                <p className="mt-0.5 text-xs text-muted-foreground truncate">{template.description}</p>
              )}
              <p className="mt-0.5 text-xs text-muted-foreground/60 truncate font-mono">
                {template.content.replace(/\n/g, " ").slice(0, 80)}
                {template.content.length > 80 ? "…" : ""}
              </p>
            </div>

            {/* Use count */}
            {template.use_count > 0 && (
              <div className="flex items-center gap-0.5 text-[10px] text-muted-foreground/60 shrink-0 mt-1">
                <TrendingUp className="w-2.5 h-2.5" />
                {template.use_count}
              </div>
            )}
          </button>
        ))}
      </div>
    </div>
  );
}
