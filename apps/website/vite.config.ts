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
    dedupe: ["react", "react-dom", "lucide-react", "recharts", "radix-ui", "@tanstack/react-virtual", "class-variance-authority", "clsx", "tailwind-merge"],
    alias: [
      { find: path.resolve(desktopSrc, "api/tauri"), replacement: path.resolve(root, "src/browser-api.ts") },
      { find: path.resolve(desktopSrc, "lib/windowChrome"), replacement: path.resolve(root, "src/browser-window-chrome.ts") },
      { find: "@", replacement: desktopSrc },
    ],
  },
  build: {
    chunkSizeWarningLimit: 1000,
  },
  server: { port: 4174, strictPort: true },
});
