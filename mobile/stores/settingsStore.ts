import { create } from "zustand";
import { settingsApi } from "../api/endpoints";
import { AppSettings } from "../lib/types";

interface SettingsState {
  settings: AppSettings | null;
  isSaving: boolean;

  fetchSettings: () => Promise<void>;
  saveSettings: (settings: AppSettings) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  settings: null,
  isSaving: false,

  fetchSettings: async () => {
    const s = await settingsApi.get();
    set({ settings: s });
  },

  saveSettings: async (settings) => {
    set({ isSaving: true });
    try {
      await settingsApi.save(settings);
      set({ settings });
    } finally {
      set({ isSaving: false });
    }
  },
}));
