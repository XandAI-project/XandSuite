import { create } from "zustand";
import { invoke } from "@/lib/tauri";
import { useSkillsStore } from "@/stores/skillsStore";

/**
 * Refresh the skills store after a package change so the connected-server list,
 * the tool count and the available-tools list stay in sync with the live
 * backend MCP servers — no app restart required. Runs alongside the backend
 * `skills_updated` event for transports where that event isn't delivered.
 */
function refreshSkills() {
  const s = useSkillsStore.getState();
  void s.fetchServers();
  void s.fetchTools();
}

// ── Types mirroring Rust structs ──────────────────────────────────────────────

export interface PackageArgSchema {
  name: string;
  label: string;
  type: "string" | "number" | "boolean" | "dynamic_select" | "file";
  required: boolean;
  placeholder: string;
  arg_prefix: string;
  /** For dynamic_select: which sibling field provides the base URL. */
  depends_on?: string;
  /** For dynamic_select: path appended to the depends_on URL to fetch options. */
  fetch_endpoint?: string;
  /** For file: allowed extensions (e.g. ["json"]). */
  file_extensions?: string[];
}

export interface OfficialPackage {
  id: string;
  name: string;
  description: string;
  category: string;
  icon: string;
  script: string;
  requires: string[];
  args_schema: PackageArgSchema[];
  installed: boolean;
  config: Record<string, string>;
}

export interface CustomPackage {
  id: string;
  name: string;
  description: string;
  /** requirements.txt-style list of pip dependencies (newline-separated). */
  requirements: string;
  created_at: string;
  installed: boolean;
}

// ── Store ─────────────────────────────────────────────────────────────────────

interface PackagesStore {
  officialPackages: OfficialPackage[];
  customPackages: CustomPackage[];
  isLoading: boolean;
  error: string | null;

  fetchOfficial: () => Promise<void>;
  fetchCustom: () => Promise<void>;

  /** Install an official package with the provided config args. */
  installPackage: (packageId: string, config: Record<string, string>) => Promise<void>;
  /** Uninstall an official package. */
  uninstallPackage: (packageId: string) => Promise<void>;

  /** Create or update a custom package script. */
  saveCustomPackage: (
    id: string,
    name: string,
    description: string,
    requirements: string,
    code: string
  ) => Promise<CustomPackage>;
  /** Read the source code of a custom package. */
  getCustomPackageCode: (id: string) => Promise<string>;
  /** Permanently delete a custom package. */
  deleteCustomPackage: (id: string) => Promise<void>;
  /** Connect a custom package as an MCP server. */
  installCustomPackage: (id: string) => Promise<void>;
  /** Disconnect a custom package's MCP server. */
  uninstallCustomPackage: (id: string) => Promise<void>;

  clearError: () => void;
}

export const usePackagesStore = create<PackagesStore>((set, get) => ({
  officialPackages: [],
  customPackages: [],
  isLoading: false,
  error: null,

  fetchOfficial: async () => {
    set({ isLoading: true, error: null });
    try {
      const packages = await invoke<OfficialPackage[]>("list_official_packages");
      set({ officialPackages: packages, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  fetchCustom: async () => {
    set({ isLoading: true, error: null });
    try {
      const packages = await invoke<CustomPackage[]>("list_custom_packages");
      set({ customPackages: packages, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
    }
  },

  installPackage: async (packageId, config) => {
    set({ isLoading: true, error: null });
    try {
      await invoke("install_package", { packageId, config });
      await get().fetchOfficial();
      refreshSkills();
      set({ isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
      throw e;
    }
  },

  uninstallPackage: async (packageId) => {
    set({ isLoading: true, error: null });
    try {
      await invoke("uninstall_package", { packageId });
      await get().fetchOfficial();
      refreshSkills();
      set({ isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
      throw e;
    }
  },

  saveCustomPackage: async (id, name, description, requirements, code) => {
    set({ isLoading: true, error: null });
    try {
      const pkg = await invoke<CustomPackage>("save_custom_package", {
        id,
        name,
        description,
        requirements,
        code,
      });
      await get().fetchCustom();
      set({ isLoading: false });
      return pkg;
    } catch (e) {
      set({ error: String(e), isLoading: false });
      throw e;
    }
  },

  getCustomPackageCode: async (id) => {
    return invoke<string>("get_custom_package_code", { id });
  },

  deleteCustomPackage: async (id) => {
    set({ isLoading: true, error: null });
    try {
      await invoke("delete_custom_package", { id });
      await get().fetchCustom();
      refreshSkills();
      set({ isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
      throw e;
    }
  },

  installCustomPackage: async (id) => {
    set({ isLoading: true, error: null });
    try {
      await invoke("install_custom_package", { id });
      await get().fetchCustom();
      refreshSkills();
      set({ isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
      throw e;
    }
  },

  uninstallCustomPackage: async (id) => {
    set({ isLoading: true, error: null });
    try {
      await invoke("uninstall_custom_package", { id });
      await get().fetchCustom();
      refreshSkills();
      set({ isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
      throw e;
    }
  },

  clearError: () => set({ error: null }),
}));
