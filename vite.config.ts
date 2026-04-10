/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],

  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },

  clearScreen: false,

  // Prevent Vite from scanning or pre-bundling anything inside the Rust
  // build artefacts directory. cargo doc produces tens-of-thousands of HTML
  // files that exhaust the OS file-handle limit (EMFILE) if Vite's dep
  // scanner accidentally follows an import into that tree.
  optimizeDeps: {
    exclude: ["src-tauri"],
  },

  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Keep the file-system watcher away from Rust build artefacts.
      ignored: ["**/src-tauri/**", "**/src-tauri/target/**"],
    },
    fs: {
      // Restrict the dev-server from serving files outside the project root.
      // This also prevents the dep scanner from following imports into
      // src-tauri/target.
      deny: ["src-tauri/target"],
    },
  },

  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
  },
}));
