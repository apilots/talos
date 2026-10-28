import { defineConfig, normalizePath } from "vite";
import { createRequire } from "node:module";
import { viteStaticCopy } from "vite-plugin-static-copy";
import react from "@vitejs/plugin-react";
import os from "node:os";
import path from "node:path";

const pdfjsDirectory = path.dirname(createRequire(import.meta.url).resolve("pdfjs-dist/package.json"));

const defaultProjectRoot =
  process.env.VITE_DEFAULT_PROJECT_ROOT ?? path.join(os.homedir(), ".talos", "workspace-talos");
const devApiProxyTarget = process.env.VITE_API_PROXY_TARGET ?? "http://127.0.0.1:8080";

export default defineConfig({
  base: process.env.TALOS_WEB_BASE ?? "/",
  define: {
    __TALOS_DEFAULT_PROJECT_ROOT__: JSON.stringify(defaultProjectRoot),
  },
  plugins: [react(), viteStaticCopy({ targets: ["cmaps", "wasm", "standard_fonts"].map(folder => ({ src: normalizePath(path.join(pdfjsDirectory, folder)), dest: "pdf" })) })],
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

          if (id.includes("@talos/sdk-browser")) {
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
    proxy: {
      "/health": {
        target: devApiProxyTarget,
        changeOrigin: true,
      },
      "/v1": {
        target: devApiProxyTarget,
        changeOrigin: true,
      },
    },
  },
});
