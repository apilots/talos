import { defineConfig, mergeConfig } from "vitest/config";
import viteConfig from "./vite.config";

export default mergeConfig(viteConfig, defineConfig({
  test: {
    // Let jsdom own per-window Storage, not Node's process-global Web Storage.
    poolOptions: {
      forks: { execArgv: ["--no-experimental-webstorage"] },
    },
  },
}));
