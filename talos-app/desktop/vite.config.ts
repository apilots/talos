import {cpSync,mkdirSync} from "node:fs";
import { builtinModules } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";
import { APP_ICONS, APP_ICON_IDS } from "../shared/app-icons";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const nodeBuiltins = builtinModules.flatMap((name) => [name, `node:${name}`]);

export default defineConfig({
  plugins: [{name:"desktop-resources",closeBundle(){
    const output=path.join(dirname,"dist/browser-window");mkdirSync(output,{recursive:true});
    cpSync(path.join(dirname,"../shared/browser-home"),path.join(output,"home"),{recursive:true});
    cpSync(path.join(dirname,"../web/public/talos-logo.svg"),path.join(output,"home/talos-logo.svg"));
    const icons = path.join(dirname, "dist/app-icons");
    mkdirSync(icons, { recursive: true });
    for (const id of APP_ICON_IDS) {
      const file = APP_ICONS[id].nativeFile;
      cpSync(path.join(dirname, "../../build/desktop/icons", file), path.join(icons, file));
    }
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
