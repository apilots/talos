import { defineConfig, normalizePath, type Plugin } from "vite";
import { mkdtemp, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { viteStaticCopy } from "vite-plugin-static-copy";
import react from "@vitejs/plugin-react";
import os from "node:os";
import path from "node:path";

import { appIconAssetPath, DEFAULT_APP_ICON_ID } from "../shared/app-icons";
import { appIconSizes } from "../shared/app-icons/assets.mjs";
import { generateAppIcons } from "../shared/app-icons/generate.mjs";

const pdfjsDirectory = path.dirname(createRequire(import.meta.url).resolve("pdfjs-dist/package.json"));

const defaultProjectRoot =
  process.env.VITE_DEFAULT_PROJECT_ROOT ?? path.join(os.homedir(), ".talos", "workspace-talos");
const devApiProxyTarget = process.env.VITE_API_PROXY_TARGET ?? "http://127.0.0.1:8080";

function appIcons(): Plugin {
  let assets: string | undefined;
  let base: string;
  return {
    name: "talos-app-icons",
    async config() {
      assets = await mkdtemp(path.join(os.tmpdir(), "talos-web-icons-"));
      try {
        await generateAppIcons("web", assets);
        return { publicDir: assets };
      } catch (error) {
        await rm(assets, { recursive: true, force: true });
        throw error;
      }
    },
    configResolved(config) {
      base = config.base;
    },
    transformIndexHtml: {
      order: "post",
      handler() {
        return [
          {
            tag: "link",
            attrs: {
              "data-app-icon-favicon": "",
              href: base + appIconAssetPath(DEFAULT_APP_ICON_ID, "favicon"),
              rel: "icon",
              sizes: `${appIconSizes.favicon}x${appIconSizes.favicon}`,
              type: "image/png",
            },
            injectTo: "head",
          },
          {
            tag: "link",
            attrs: {
              "data-app-icon-touch": "",
              href: base + appIconAssetPath(DEFAULT_APP_ICON_ID, "appleTouchIcon"),
              rel: "apple-touch-icon",
            },
            injectTo: "head",
          },
        ];
      },
    },
    async closeBundle() {
      if (assets) await rm(assets, { recursive: true, force: true });
    },
  };
}

export default defineConfig({
  base: process.env.TALOS_WEB_BASE ?? "/",
  define: {
    __TALOS_DEFAULT_PROJECT_ROOT__: JSON.stringify(defaultProjectRoot),
  },
  plugins: [react(), appIcons(), viteStaticCopy({ targets: ["cmaps", "wasm", "standard_fonts"].map(folder => ({ src: normalizePath(path.join(pdfjsDirectory, folder)), dest: "pdf" })) })],
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
