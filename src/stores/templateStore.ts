import { create } from "zustand";
import { invoke } from "@/lib/tauri";
import type { PromptTemplate, CreateTemplateInput, UpdateTemplateInput } from "@/lib/tauri";

interface TemplateState {
  templates: PromptTemplate[];
  loading: boolean;
  fetchTemplates: () => Promise<void>;
  createTemplate: (input: CreateTemplateInput) => Promise<PromptTemplate>;
  updateTemplate: (input: UpdateTemplateInput) => Promise<PromptTemplate>;
  deleteTemplate: (templateId: string) => Promise<void>;
  incrementUse: (templateId: string) => Promise<void>;
}

export const useTemplateStore = create<TemplateState>((set, _get) => ({
  templates: [],
  loading: false,

  fetchTemplates: async () => {
    set({ loading: true });
    try {
      const templates = await invoke<PromptTemplate[]>("list_templates");
      set({ templates });
    } catch (e) {
      console.error("Failed to fetch templates:", e);
    } finally {
      set({ loading: false });
    }
  },

  createTemplate: async (input: CreateTemplateInput) => {
    const template = await invoke<PromptTemplate>("create_template", { input });
    set((state) => ({ templates: [template, ...state.templates] }));
    return template;
  },

  updateTemplate: async (input: UpdateTemplateInput) => {
    const updated = await invoke<PromptTemplate>("update_template", { input });
    set((state) => ({
      templates: state.templates.map((t) => (t.id === updated.id ? updated : t)),
    }));
    return updated;
  },

  deleteTemplate: async (templateId: string) => {
    await invoke("delete_template", { templateId });
    set((state) => ({
      templates: state.templates.filter((t) => t.id !== templateId),
    }));
  },

  incrementUse: async (templateId: string) => {
    await invoke("increment_template_use", { templateId });
    set((state) => ({
      templates: state.templates.map((t) =>
        t.id === templateId ? { ...t, use_count: t.use_count + 1 } : t
      ),
    }));
  },
}));
