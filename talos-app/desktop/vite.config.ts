import {cpSync,mkdirSync} from "node:fs";
import { builtinModules } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const nodeBuiltins = builtinModules.flatMap((name) => [name, `node:${name}`]);

export default defineConfig({
  plugins: [{name:"browser-window-resources",closeBundle(){
    const output=path.join(dirname,"dist/browser-window");mkdirSync(output,{recursive:true});
    cpSync(path.join(dirname,"../shared/browser-home"),path.join(output,"home"),{recursive:true});
    cpSync(path.join(dirname,"../web/public/talos-logo.svg"),path.join(output,"home/talos-logo.svg"));
  }}],
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
      external: ["electron", "app-path", "open", ...nodeBuiltins],
    },
  },
});
