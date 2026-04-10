import { useEffect, useRef, useState } from "react";
import {
  X,
  ImagePlus,
  Smile,
  Loader2,
  Brain,
  Database,
  Cpu,
} from "lucide-react";
import { usePersonaStore } from "@/stores/personaStore";
import { useRagStore } from "@/stores/ragStore";
import { useModelStore } from "@/stores/modelStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import type { Persona, CreatePersonaInput, UpdatePersonaInput } from "@/lib/tauri";

// ── Emoji quick-picks ──────────────────────────────────────────────────────────
const EMOJI_PICKS = [
  "🤖","🧑‍💻","🧑‍🔬","🎨","📚","🧠","💡","🔮","🦊","🐉",
  "🌟","⚡","🔥","🌊","🎭","🦸","🧙","🐺","🦁","🐻",
];

interface PersonaEditorProps {
  open: boolean;
  persona: Persona | null;
  onClose: () => void;
}

interface FormState {
  name: string;
  description: string;
  avatar: string;
  system_prompt: string;
  model_id: string;
  rag_collection_ids: string[];
  memory_enabled: boolean;
}

const EMPTY_FORM: FormState = {
  name: "",
  description: "",
  avatar: "",
  system_prompt: "",
  model_id: "",
  rag_collection_ids: [],
  memory_enabled: false,
};

export function PersonaEditor({ open, persona, onClose }: PersonaEditorProps) {
  const { createPersona, updatePersona } = usePersonaStore();
  const { collections, fetchCollections } = useRagStore();
  const { downloadedModels, fetchDownloadedModels } = useModelStore();

  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showEmojiPicker, setShowEmojiPicker] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const isEdit = !!persona;

  // Populate form when editing
  useEffect(() => {
    if (persona) {
      setForm({
        name: persona.name,
        description: persona.description ?? "",
        avatar: persona.avatar ?? "",
        system_prompt: persona.system_prompt,
        model_id: persona.model_id ?? "",
        rag_collection_ids: persona.rag_collection_ids,
        memory_enabled: persona.memory_enabled,
      });
    } else {
      setForm(EMPTY_FORM);
    }
    setError(null);
  }, [persona, open]);

  useEffect(() => {
    if (open) {
      fetchCollections();
      fetchDownloadedModels();
    }
  }, [open, fetchCollections, fetchDownloadedModels]);

  const patch = (updates: Partial<FormState>) =>
    setForm((prev) => ({ ...prev, ...updates }));

  const handleAvatarFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      patch({ avatar: reader.result as string });
    };
    reader.readAsDataURL(file);
  };

  const toggleRagCollection = (id: string) => {
    patch({
      rag_collection_ids: form.rag_collection_ids.includes(id)
        ? form.rag_collection_ids.filter((c) => c !== id)
        : [...form.rag_collection_ids, id],
    });
  };

  const handleSave = async () => {
    if (!form.name.trim()) {
      setError("Name is required.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      if (isEdit && persona) {
        const input: UpdatePersonaInput = {
          id: persona.id,
          name: form.name.trim(),
          description: form.description.trim() || undefined,
          avatar: form.avatar || undefined,
          system_prompt: form.system_prompt,
          model_id: form.model_id || undefined,
          rag_collection_ids: form.rag_collection_ids,
          memory_enabled: form.memory_enabled,
        };
        await updatePersona(input);
      } else {
        const input: CreatePersonaInput = {
          name: form.name.trim(),
          description: form.description.trim() || undefined,
          avatar: form.avatar || undefined,
          system_prompt: form.system_prompt,
          model_id: form.model_id || undefined,
          rag_collection_ids: form.rag_collection_ids,
          memory_enabled: form.memory_enabled,
        };
        await createPersona(input);
      }
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (!open) return null;

  const visibleCollections = collections.filter((c) => c.id !== "xand_internal_memory");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="relative flex h-[90vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-border bg-background shadow-2xl">
        {/* Header */}
        <div className="flex shrink-0 items-center justify-between border-b border-border px-6 py-4">
          <h2 className="text-lg font-semibold">
            {isEdit ? "Edit Persona" : "New Persona"}
          </h2>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <ScrollArea className="flex-1">
          <div className="flex flex-col gap-6 p-6">
            {/* Avatar + Name */}
            <div className="flex items-start gap-5">
              {/* Avatar picker */}
              <div className="flex flex-col items-center gap-2 shrink-0">
                <div
                  className="relative flex h-20 w-20 cursor-pointer items-center justify-center overflow-hidden rounded-full border-2 border-border bg-secondary transition-all hover:border-primary/50"
                  onClick={() => setShowEmojiPicker((v) => !v)}
                >
                  {form.avatar ? (
                    form.avatar.startsWith("data:") ? (
                      <img src={form.avatar} alt="avatar" className="h-full w-full object-cover" />
                    ) : (
                      <span className="text-4xl">{form.avatar}</span>
                    )
                  ) : (
                    <span className="text-3xl text-muted-foreground">🤖</span>
                  )}
                  <div className="absolute inset-0 flex items-center justify-center bg-black/40 opacity-0 transition-opacity hover:opacity-100">
                    <Smile className="w-6 h-6 text-white" />
                  </div>
                </div>
                <button
                  className="flex items-center gap-1 rounded px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
                  onClick={() => fileInputRef.current?.click()}
                >
                  <ImagePlus className="w-3 h-3" />
                  Photo
                </button>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  className="hidden"
                  onChange={handleAvatarFile}
                />
              </div>

              {/* Emoji picker dropdown */}
              {showEmojiPicker && (
                <div className="absolute z-10 mt-24 rounded-xl border border-border bg-popover p-3 shadow-lg">
                  <div className="grid grid-cols-10 gap-1">
                    {EMOJI_PICKS.map((emoji) => (
                      <button
                        key={emoji}
                        className="rounded p-1.5 text-xl hover:bg-secondary"
                        onClick={() => {
                          patch({ avatar: emoji });
                          setShowEmojiPicker(false);
                        }}
                      >
                        {emoji}
                      </button>
                    ))}
                  </div>
                  <button
                    className="mt-2 w-full rounded px-2 py-1 text-xs text-muted-foreground hover:bg-secondary"
                    onClick={() => patch({ avatar: "" })}
                  >
                    Clear avatar
                  </button>
                </div>
              )}

              {/* Name + description */}
              <div className="flex flex-1 flex-col gap-3">
                <div>
                  <label className="mb-1.5 block text-sm font-medium">Name *</label>
                  <Input
                    placeholder="e.g. Code Mentor, Creative Writer…"
                    value={form.name}
                    onChange={(e) => patch({ name: e.target.value })}
                    autoFocus
                  />
                </div>
                <div>
                  <label className="mb-1.5 block text-sm font-medium text-muted-foreground">
                    Description
                  </label>
                  <Input
                    placeholder="Short description shown on the card"
                    value={form.description}
                    onChange={(e) => patch({ description: e.target.value })}
                  />
                </div>
              </div>
            </div>

            {/* System prompt */}
            <div>
              <label className="mb-1.5 block text-sm font-medium">
                System Prompt
              </label>
              <Textarea
                className="min-h-[140px] font-mono text-sm resize-y"
                placeholder="You are a helpful assistant specialised in…"
                value={form.system_prompt}
                onChange={(e) => patch({ system_prompt: e.target.value })}
              />
              <p className="mt-1.5 text-xs text-muted-foreground">
                Defines the persona's personality, knowledge, and rules. Injected at the start of every conversation.
              </p>
            </div>

            {/* Model picker */}
            <div>
              <label className="mb-1.5 flex items-center gap-1.5 text-sm font-medium">
                <Cpu className="w-3.5 h-3.5 text-muted-foreground" />
                Preferred Model
              </label>
              <select
                className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
                value={form.model_id}
                onChange={(e) => patch({ model_id: e.target.value })}
              >
                <option value="">App default (currently loaded model)</option>
                {downloadedModels.map((m) => (
                  <option key={m.path} value={m.path}>
                    {m.filename}
                  </option>
                ))}
              </select>
              <p className="mt-1.5 text-xs text-muted-foreground">
                The model shown here will be highlighted when starting a chat with this persona.
              </p>
            </div>

            {/* RAG collections */}
            {visibleCollections.length > 0 && (
              <div>
                <label className="mb-2 flex items-center gap-1.5 text-sm font-medium">
                  <Database className="w-3.5 h-3.5 text-muted-foreground" />
                  Default Knowledge Bases
                </label>
                <div className="flex flex-wrap gap-2">
                  {visibleCollections.map((col) => {
                    const active = form.rag_collection_ids.includes(col.id);
                    return (
                      <button
                        key={col.id}
                        onClick={() => toggleRagCollection(col.id)}
                        className={cn(
                          "flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs transition-all",
                          active
                            ? "border-primary bg-primary/10 text-primary"
                            : "border-border bg-secondary/40 text-muted-foreground hover:border-primary/40 hover:text-foreground"
                        )}
                      >
                        <Database className="w-3 h-3" />
                        {col.name}
                        {active && <span className="ml-0.5 text-primary">✓</span>}
                      </button>
                    );
                  })}
                </div>
                <p className="mt-1.5 text-xs text-muted-foreground">
                  Selected collections will be searched automatically for every message.
                </p>
              </div>
            )}

            {/* Memory toggle */}
            <div className="flex items-start gap-3 rounded-xl border border-border bg-secondary/20 p-4">
              <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-purple-500/10">
                <Brain className="w-4.5 h-4.5 text-purple-400" />
              </div>
              <div className="flex-1">
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium">Persona Memory</p>
                    <p className="text-xs text-muted-foreground">
                      Remember facts from past conversations with this persona.
                    </p>
                  </div>
                  <button
                    role="switch"
                    aria-checked={form.memory_enabled}
                    onClick={() => patch({ memory_enabled: !form.memory_enabled })}
                    className={cn(
                      "relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      form.memory_enabled ? "bg-purple-500" : "bg-secondary"
                    )}
                  >
                    <span
                      className={cn(
                        "pointer-events-none block h-4 w-4 rounded-full bg-background shadow-md transition-transform",
                        form.memory_enabled ? "translate-x-4" : "translate-x-0"
                      )}
                    />
                  </button>
                </div>
              </div>
            </div>

            {error && (
              <p className="rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive">
                {error}
              </p>
            )}
          </div>
        </ScrollArea>

        {/* Footer */}
        <div className="flex shrink-0 items-center justify-end gap-3 border-t border-border px-6 py-4">
          <Button variant="outline" onClick={onClose} disabled={saving}>
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={saving} className="gap-2 min-w-[100px]">
            {saving ? (
              <>
                <Loader2 className="w-4 h-4 animate-spin" />
                Saving…
              </>
            ) : isEdit ? (
              "Save changes"
            ) : (
              "Create persona"
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
