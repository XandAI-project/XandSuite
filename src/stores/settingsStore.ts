import { create } from "zustand";
import { invoke } from "@/lib/tauri";
import type { AppSettings } from "@/lib/tauri";

interface SettingsState {
  settings: AppSettings | null;
  fetchSettings: () => Promise<void>;
  saveSettings: (patch: Partial<AppSettings>) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,

  fetchSettings: async () => {
    try {
      const s = await invoke<AppSettings>("get_settings");
      set({ settings: s });
    } catch (e) {
      console.error("Failed to fetch settings:", e);
    }
  },

  saveSettings: async (patch: Partial<AppSettings>) => {
    try {
      // If the store hasn't loaded settings yet, fetch from backend first so
      // we don't accidentally wipe fields that weren't included in the patch.
      let current = get().settings;
      if (!current) {
        current = await invoke<AppSettings>("get_settings");
        set({ settings: current });
      }
      const merged = { ...current, ...patch } as AppSettings;
      await invoke("save_settings", { settings: merged });
      set({ settings: merged });
    } catch (e) {
      console.error("Failed to save settings:", e);
      throw e;
    }
  },
}));
