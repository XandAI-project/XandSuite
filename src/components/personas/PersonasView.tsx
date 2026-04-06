import { useEffect, useState } from "react";
import {
  Plus,
  MessageSquare,
  Pencil,
  Trash2,
  Brain,
  Database,
  Cpu,
  UserCircle2,
  Loader2,
} from "lucide-react";
import { useNavigate } from "react-router-dom";
import { usePersonaStore } from "@/stores/personaStore";
import { useChatStore } from "@/stores/chatStore";
import { useRagStore } from "@/stores/ragStore";
import { useModelStore } from "@/stores/modelStore";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import type { Persona } from "@/lib/tauri";
import { PersonaEditor } from "./PersonaEditor";

function PersonaAvatar({ persona, size = "md" }: { persona: Persona; size?: "sm" | "md" | "lg" }) {
  const sizes = {
    sm: "w-10 h-10 text-lg",
    md: "w-16 h-16 text-3xl",
    lg: "w-20 h-20 text-4xl",
  };
  const initials = persona.name
    .split(" ")
    .map((w) => w[0])
    .slice(0, 2)
    .join("")
    .toUpperCase();

  if (persona.avatar) {
    if (persona.avatar.startsWith("data:")) {
      return (
        <img
          src={persona.avatar}
          alt={persona.name}
          className={cn("rounded-full object-cover shrink-0", sizes[size])}
        />
      );
    }
    // Emoji avatar
    return (
      <div className={cn("flex items-center justify-center rounded-full bg-secondary shrink-0", sizes[size])}>
        <span>{persona.avatar}</span>
      </div>
    );
  }

  return (
    <div
      className={cn(
        "flex items-center justify-center rounded-full bg-primary/10 text-primary font-semibold shrink-0",
        sizes[size]
      )}
    >
      {initials || <UserCircle2 className="w-1/2 h-1/2" />}
    </div>
  );
}

function PersonaCard({
  persona,
  onChat,
  onEdit,
  onDelete,
}: {
  persona: Persona;
  onChat: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { downloadedModels } = useModelStore();
  const { collections } = useRagStore();
  const [deleting, setDeleting] = useState(false);

  const modelName = persona.model_id
    ? downloadedModels.find((m) => m.path === persona.model_id)?.filename ??
      persona.model_id.split(/[\\/]/).pop() ??
      "Custom model"
    : "App default";

  const ragCount = persona.rag_collection_ids.filter(
    (id) => collections.some((c) => c.id === id)
  ).length;

  const handleDelete = async () => {
    if (!confirm(`Delete persona "${persona.name}"?`)) return;
    setDeleting(true);
    try {
      await onDelete();
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="group relative flex flex-col gap-4 rounded-xl border border-border bg-card p-5 transition-all hover:border-primary/40 hover:shadow-md hover:shadow-primary/5">
      {/* Header */}
      <div className="flex items-start gap-4">
        <PersonaAvatar persona={persona} size="md" />
        <div className="min-w-0 flex-1">
          <h3 className="truncate font-semibold text-foreground leading-tight">{persona.name}</h3>
          {persona.description && (
            <p className="mt-0.5 line-clamp-2 text-sm text-muted-foreground">
              {persona.description}
            </p>
          )}
        </div>
      </div>

      {/* Badges */}
      <div className="flex flex-wrap gap-1.5">
        <Badge variant="secondary" className="gap-1 text-xs">
          <Cpu className="w-3 h-3" />
          {modelName}
        </Badge>
        {ragCount > 0 && (
          <Badge variant="secondary" className="gap-1 text-xs">
            <Database className="w-3 h-3" />
            {ragCount} collection{ragCount > 1 ? "s" : ""}
          </Badge>
        )}
        {persona.memory_enabled && (
          <Badge variant="secondary" className="gap-1 text-xs text-purple-400 border-purple-500/30">
            <Brain className="w-3 h-3" />
            Memory
          </Badge>
        )}
      </div>

      {/* System prompt preview */}
      {persona.system_prompt && (
        <p className="line-clamp-2 rounded-md bg-secondary/40 px-3 py-2 text-xs text-muted-foreground font-mono">
          {persona.system_prompt}
        </p>
      )}

      {/* Actions */}
      <div className="flex items-center gap-2 pt-1">
        <Button size="sm" className="flex-1 gap-1.5" onClick={onChat}>
          <MessageSquare className="w-3.5 h-3.5" />
          Chat
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="gap-1.5"
          onClick={onEdit}
        >
          <Pencil className="w-3.5 h-3.5" />
          Edit
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="text-destructive hover:text-destructive hover:bg-destructive/10"
          onClick={handleDelete}
          disabled={deleting}
        >
          {deleting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Trash2 className="w-3.5 h-3.5" />}
        </Button>
      </div>
    </div>
  );
}

export function PersonasView() {
  const navigate = useNavigate();
  const { personas, loading, fetchPersonas, deletePersona } = usePersonaStore();
  const { createConversation, openConversation } = useChatStore();
  const { fetchDownloadedModels } = useModelStore();
  const { fetchCollections } = useRagStore();

  const [editorOpen, setEditorOpen] = useState(false);
  const [editingPersona, setEditingPersona] = useState<Persona | null>(null);

  useEffect(() => {
    fetchPersonas();
    fetchDownloadedModels();
    fetchCollections();
  }, [fetchPersonas, fetchDownloadedModels, fetchCollections]);

  const handleChat = async (persona: Persona) => {
    const conv = await createConversation(persona.system_prompt || undefined, persona.id);
    await openConversation(conv.id);
    navigate("/chat");
  };

  const handleEdit = (persona: Persona) => {
    setEditingPersona(persona);
    setEditorOpen(true);
  };

  const handleNew = () => {
    setEditingPersona(null);
    setEditorOpen(true);
  };

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex shrink-0 items-center justify-between border-b border-border px-6 py-4">
        <div>
          <h1 className="text-lg font-semibold">Personas</h1>
          <p className="text-sm text-muted-foreground">
            Create AI characters with custom personalities, models, and knowledge bases.
          </p>
        </div>
        <Button onClick={handleNew} className="gap-2 shrink-0">
          <Plus className="w-4 h-4" />
          New Persona
        </Button>
      </div>

      {/* Content */}
      <ScrollArea className="flex-1">
        <div className="p-6">
          {loading ? (
            <div className="flex items-center justify-center py-16 text-muted-foreground gap-2">
              <Loader2 className="w-5 h-5 animate-spin" />
              Loading personas…
            </div>
          ) : personas.length === 0 ? (
            <div className="flex flex-col items-center justify-center gap-4 py-20 text-center">
              <div className="flex h-16 w-16 items-center justify-center rounded-full bg-secondary">
                <UserCircle2 className="w-8 h-8 text-muted-foreground" />
              </div>
              <div>
                <p className="font-medium text-foreground">No personas yet</p>
                <p className="mt-1 text-sm text-muted-foreground">
                  Create a persona to start a tailored conversation experience.
                </p>
              </div>
              <Button onClick={handleNew} className="gap-2">
                <Plus className="w-4 h-4" />
                Create your first persona
              </Button>
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              {personas.map((persona) => (
                <PersonaCard
                  key={persona.id}
                  persona={persona}
                  onChat={() => handleChat(persona)}
                  onEdit={() => handleEdit(persona)}
                  onDelete={() => deletePersona(persona.id)}
                />
              ))}
            </div>
          )}
        </div>
      </ScrollArea>

      {/* Editor modal */}
      <PersonaEditor
        open={editorOpen}
        persona={editingPersona}
        onClose={() => {
          setEditorOpen(false);
          setEditingPersona(null);
        }}
      />
    </div>
  );
}
