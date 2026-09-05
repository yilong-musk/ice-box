import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const root = path.dirname(fileURLToPath(import.meta.url));
const desktopSrc = path.resolve(root, "../desktop/src");

export default defineConfig({
  base: "./",
  plugins: [
    {
      name: "website-desktop-browser-adapters",
      enforce: "pre",
      resolveId(source, importer) {
        if (!importer?.includes(`${path.sep}apps${path.sep}desktop${path.sep}src${path.sep}`)) return null;
        if (source === "./api/tauri" || source.endsWith("/api/tauri")) return path.resolve(root, "src/browser-api.ts");
        if (source === "@/lib/windowChrome") return path.resolve(root, "src/browser-window-chrome.ts");
        return null;
      },
    },
    react(),
    tailwindcss(),
  ],
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
    rollupOptions: {
      input: {
        main: path.resolve(root, "index.html"),
        demo: path.resolve(root, "demo.html"),
      },
    },
  },
  server: { port: 4174, strictPort: true },
});
