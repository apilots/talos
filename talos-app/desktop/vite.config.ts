import { builtinModules } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";
import { generateAppIcons, prepareBrowserHome } from "../shared/app-icons/generate.mjs";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const nodeBuiltins = builtinModules.flatMap((name) => [name, `node:${name}`]);

export default defineConfig({
  plugins: [{
    name: "desktop-resources",
    async closeBundle() {
      const output = path.join(dirname, "dist");
      await Promise.all([
        generateAppIcons("desktop", output),
        prepareBrowserHome(path.join(output, "browser-window/home")),
      ]);
    },
  }],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "node22",
    lib: {
      entry: {
        main: path.resolve(dirname, "src/main.ts"),
      },
      formats: ["es"],
      fileName: () => "main.js",
    },
    rollupOptions: {
      external: ["electron", "app-path", "open", "undici", ...nodeBuiltins],
    },
  },
});
