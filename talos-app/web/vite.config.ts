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
