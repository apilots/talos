import { defineConfig } from "../../talos-app/desktop/node_modules/vite";
import { fileURLToPath } from "node:url";

export default defineConfig({
  build: {
    target: "node22", ssr: fileURLToPath(new URL("./desktop-recovery.ts", import.meta.url)),
    outDir: process.env.TALOS_SCENARIO_REPORT_DIR, emptyOutDir: false,
    rollupOptions: { output: { entryFileNames: "desktop-recovery.mjs" } },
  },
  ssr: { noExternal: ["@talos/sdk-core", "@talos/sdk-app-server"] },
});
