import { create } from "zustand";
import { invoke } from "@/lib/tauri";
import type { Persona, CreatePersonaInput, UpdatePersonaInput } from "@/lib/tauri";

interface PersonaState {
  personas: Persona[];
  loading: boolean;
  fetchPersonas: () => Promise<void>;
  createPersona: (input: CreatePersonaInput) => Promise<Persona>;
  updatePersona: (input: UpdatePersonaInput) => Promise<Persona>;
  deletePersona: (personaId: string) => Promise<void>;
}

export const usePersonaStore = create<PersonaState>((set, _get) => ({
  personas: [],
  loading: false,

  fetchPersonas: async () => {
    set({ loading: true });
    try {
      const personas = await invoke<Persona[]>("list_personas");
      set({ personas });
    } catch (e) {
      console.error("Failed to fetch personas:", e);
    } finally {
      set({ loading: false });
    }
  },

  createPersona: async (input: CreatePersonaInput) => {
    const persona = await invoke<Persona>("create_persona", { input });
    set((state) => ({ personas: [...state.personas, persona] }));
    return persona;
  },

  updatePersona: async (input: UpdatePersonaInput) => {
    const updated = await invoke<Persona>("update_persona", { input });
    set((state) => ({
      personas: state.personas.map((p) => (p.id === updated.id ? updated : p)),
    }));
    return updated;
  },

  deletePersona: async (personaId: string) => {
    await invoke("delete_persona", { personaId });
    set((state) => ({
      personas: state.personas.filter((p) => p.id !== personaId),
    }));
  },
}));
