// SPDX-License-Identifier: GPL-3.0-or-later

import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

const root = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@platform/api": path.resolve(root, "src/api/tauri.ts"),
      "@platform/windowChrome": path.resolve(root, "src/lib/windowChrome.ts"),
      "@platform/shell": path.resolve(root, "src/lib/shell.ts"),
      "@": path.resolve(root, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    // Parallel jsdom + Recharts workers starve waitFor timers on WSL.
    fileParallelism: false,
    testTimeout: 30_000,
    hookTimeout: 30_000,
  },
});
