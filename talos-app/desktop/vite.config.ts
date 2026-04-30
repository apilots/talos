import { builtinModules } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const nodeBuiltins = builtinModules.flatMap((name) => [name, `node:${name}`]);

export default defineConfig({
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "node22",
    lib: {
      entry: {
        main: path.resolve(dirname, "src/main.ts"),
        preload: path.resolve(dirname, "src/preload.ts"),
      },
      formats: ["es"],
      fileName: (_format, entryName) => `${entryName}.js`,
    },
    rollupOptions: {
      external: ["electron", ...nodeBuiltins],
    },
  },
});
