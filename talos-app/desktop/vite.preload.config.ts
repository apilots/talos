import { builtinModules } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const nodeBuiltins = builtinModules.flatMap((name) => [name, `node:${name}`]);

export default defineConfig({
  build: {
    outDir: "dist",
    emptyOutDir: false,
    target: "node22",
    lib: {
      entry: {
        preload: path.resolve(dirname, "src/preload.ts"),
        browserNavigation: path.resolve(dirname,"src/browser-navigation-preload.ts"),
      },
      formats: ["cjs"],
      fileName: (_format,entryName) => entryName === "preload" ? "preload.cjs" : "browser-window/preload.cjs",
    },
    rollupOptions: {
      external: ["electron", ...nodeBuiltins],
    },
  },
});
