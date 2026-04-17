import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import os from "node:os";
import path from "node:path";

const defaultProjectRoot =
  process.env.VITE_DEFAULT_PROJECT_ROOT ?? path.join(os.homedir(), ".talos", "workspace-talos");

export default defineConfig({
  define: {
    __TALOS_DEFAULT_PROJECT_ROOT__: JSON.stringify(defaultProjectRoot),
  },
  plugins: [react()],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) {
            return undefined;
          }

          if (
            id.includes("react-markdown") ||
            id.includes("remark-gfm") ||
            id.includes("rehype-highlight") ||
            id.includes("highlight.js") ||
            id.includes("mdast-util") ||
            id.includes("micromark") ||
            id.includes("unified") ||
            id.includes("unist")
          ) {
            return "markdown-vendor";
          }

          if (id.includes("react-router-dom") || id.includes("@tanstack/react-query")) {
            return "app-vendor";
          }

          if (id.includes("@talos/sdk")) {
            return "sdk-vendor";
          }

          return undefined;
        },
      },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 5173,
    host: "127.0.0.1",
  },
});
