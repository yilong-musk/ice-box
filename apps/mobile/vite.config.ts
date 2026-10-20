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

export default defineConfig(({ mode }) => {
  // `capture` renders the phone shell against the Live Demo browser mock for
  // the README screenshots; regular dev/build keeps the Tauri transport.
  const capture = mode === "capture";

  return {
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
        {
          find: "@platform/api",
          replacement: capture
            ? path.resolve(root, "src/capture-api.ts")
            : path.resolve(root, "src/mobile-api.ts"),
        },
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
      outDir: capture ? "dist-capture" : "dist",
      emptyOutDir: true,
      chunkSizeWarningLimit: 1000,
      rollupOptions: capture
        ? { input: { capture: path.resolve(root, "capture.html") } }
        : undefined,
    },
  };
});
