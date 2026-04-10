import { useEffect, useRef, useState } from "react";
import { X, LayoutTemplate } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { PromptTemplate } from "@/lib/tauri";

const VAR_REGEX = /\{\{(\w+)\}\}/g;

/** Extract unique variable names from a template string. */
function extractVariables(content: string): string[] {
  const seen = new Set<string>();
  const vars: string[] = [];
  let m: RegExpExecArray | null;
  const re = new RegExp(VAR_REGEX.source, "g");
  while ((m = re.exec(content)) !== null) {
    if (!seen.has(m[1])) {
      seen.add(m[1]);
      vars.push(m[1]);
    }
  }
  return vars;
}

/** Replace all `{{varName}}` tokens with the provided values. */
export function fillVariables(content: string, values: Record<string, string>): string {
  return content.replace(/\{\{(\w+)\}\}/g, (_, name) => values[name] ?? `{{${name}}}`);
}

interface Props {
  template: PromptTemplate | null;
  onConfirm: (filledText: string) => void;
  onClose: () => void;
}

export function VariableFiller({ template, onConfirm, onClose }: Props) {
  const variables = template ? extractVariables(template.content) : [];
  const [values, setValues] = useState<Record<string, string>>({});
  const firstInputRef = useRef<HTMLInputElement>(null);

  // Reset when template changes
  useEffect(() => {
    const initial: Record<string, string> = {};
    variables.forEach((v) => (initial[v] = ""));
    setValues(initial);
    setTimeout(() => firstInputRef.current?.focus(), 50);
  }, [template?.id]);

  const handleConfirm = () => {
    if (!template) return;
    onConfirm(fillVariables(template.content, values));
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleConfirm();
    } else if (e.key === "Escape") {
      onClose();
    }
  };

  if (!template) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="w-full max-w-md rounded-xl border border-border bg-background shadow-2xl">
        {/* Header */}
        <div className="flex items-center gap-3 border-b border-border px-5 py-4">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary/10">
            <LayoutTemplate className="w-4 h-4 text-primary" />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-sm font-semibold truncate">{template.title}</p>
            {template.description && (
              <p className="text-xs text-muted-foreground truncate">{template.description}</p>
            )}
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Variable inputs */}
        <div className="flex flex-col gap-4 p-5" onKeyDown={handleKeyDown}>
          {variables.length === 0 ? (
            <p className="text-sm text-muted-foreground text-center py-2">
              This template has no variables. It will be inserted as-is.
            </p>
          ) : (
            variables.map((varName, idx) => (
              <div key={varName}>
                <label className="mb-1.5 block text-sm font-medium">
                  {varName.charAt(0).toUpperCase() + varName.slice(1).replace(/_/g, " ")}
                  <span className="ml-1.5 font-mono text-xs text-amber-400/80">{`{{${varName}}}`}</span>
                </label>
                <Input
                  ref={idx === 0 ? firstInputRef : undefined}
                  placeholder={`Enter ${varName}…`}
                  value={values[varName] ?? ""}
                  onChange={(e) =>
                    setValues((prev) => ({ ...prev, [varName]: e.target.value }))
                  }
                />
              </div>
            ))
          )}

          {/* Preview */}
          {variables.length > 0 && (
            <div className="rounded-lg bg-secondary/40 p-3">
              <p className="mb-1 text-xs font-medium text-muted-foreground">Preview</p>
              <p className="text-xs text-foreground/80 whitespace-pre-wrap line-clamp-4 font-mono">
                {fillVariables(template.content, values)}
              </p>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 border-t border-border px-5 py-4">
          <Button variant="outline" size="sm" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" onClick={handleConfirm}>
            Insert template
          </Button>
        </div>
      </div>
    </div>
  );
}
