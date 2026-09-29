// SPDX-License-Identifier: GPL-3.0-or-later

import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const root = path.dirname(fileURLToPath(import.meta.url));
const desktopSrc = path.resolve(root, "../desktop/src");

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  // Relative asset URLs load inside the Android WebView asset origin.
  base: "./",
  plugins: [react(), tailwindcss()],
  resolve: {
    dedupe: [
      "react",
      "react-dom",
      "lucide-react",
      "recharts",
      "radix-ui",
      "class-variance-authority",
      "clsx",
      "tailwind-merge",
    ],
    alias: [
      { find: "@platform/api", replacement: path.resolve(root, "src/mobile-api.ts") },
      {
        find: "@platform/windowChrome",
        replacement: path.resolve(root, "src/window-chrome.ts"),
      },
      { find: "@platform/shell", replacement: path.resolve(root, "src/shell.ts") },
      { find: "@", replacement: desktopSrc },
    ],
  },
  clearScreen: false,
  server: {
    port: 1422,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1423,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 1000,
  },
});
