import AsyncStorage from "@react-native-async-storage/async-storage";
import { create } from "zustand";
import { STORAGE_KEYS } from "../api/client";
import { settingsApi } from "../api/endpoints";
import { sseManager } from "../api/sse";

interface ConnectionState {
  host: string;
  token: string | null;
  isConnected: boolean;
  isChecking: boolean;
  error: string | null;

  setHost: (host: string, token?: string | null) => Promise<void>;
  checkConnection: () => Promise<boolean>;
  loadSaved: () => Promise<void>;
  disconnect: () => void;
}

export const useConnectionStore = create<ConnectionState>((set, get) => ({
  host: "",
  token: null,
  isConnected: false,
  isChecking: false,
  error: null,

  loadSaved: async () => {
    const [host, token] = await Promise.all([
      AsyncStorage.getItem(STORAGE_KEYS.HOST),
      AsyncStorage.getItem(STORAGE_KEYS.TOKEN),
    ]);
    if (host) {
      set({ host, token: token || null });
    }
  },

  setHost: async (host, token = null) => {
    set({ host, token });
    await Promise.all([
      AsyncStorage.setItem(STORAGE_KEYS.HOST, host),
      token
        ? AsyncStorage.setItem(STORAGE_KEYS.TOKEN, token)
        : AsyncStorage.removeItem(STORAGE_KEYS.TOKEN),
    ]);
  },

  checkConnection: async () => {
    set({ isChecking: true, error: null });
    try {
      await settingsApi.get();
      set({ isConnected: true, isChecking: false });
      sseManager.connect();
      return true;
    } catch (e: unknown) {
      const message = e instanceof Error ? e.message : String(e);
      set({ isConnected: false, isChecking: false, error: message });
      return false;
    }
  },

  disconnect: () => {
    sseManager.stop();
    set({ isConnected: false });
  },
}));
