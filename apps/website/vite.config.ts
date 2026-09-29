// SPDX-License-Identifier: GPL-3.0-or-later

import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const root = path.dirname(fileURLToPath(import.meta.url));
const desktopSrc = path.resolve(root, "../desktop/src");

export default defineConfig({
  base: "./",
  plugins: [react(), tailwindcss()],
  resolve: {
    dedupe: ["react", "react-dom", "lucide-react", "recharts", "radix-ui", "class-variance-authority", "clsx", "tailwind-merge"],
    alias: [
      { find: "@platform/api", replacement: path.resolve(root, "src/browser-api.ts") },
      { find: "@platform/windowChrome", replacement: path.resolve(root, "src/browser-window-chrome.ts") },
      { find: "@platform/shell", replacement: path.resolve(desktopSrc, "lib/shell.ts") },
      { find: "@", replacement: desktopSrc },
    ],
  },
  build: {
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      input: {
        main: path.resolve(root, "index.html"),
        demo: path.resolve(root, "demo.html"),
      },
    },
  },
  server: { port: 4174, strictPort: true },
});
