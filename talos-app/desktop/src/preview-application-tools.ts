import { app } from "electron";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

type ApplicationTools = {
  appPath: typeof import("app-path").default;
  open: typeof import("open").default;
  openApp: typeof import("open").openApp;
};
let tools: Promise<ApplicationTools> | undefined;

/** These packages resolve native resources relative to their module. Load the
 * complete production dependencies from their real unpacked directory in ASAR
 * builds; spawning from a virtual archive cwd is unsupported by Electron. */
export function previewApplicationTools(): Promise<ApplicationTools> {
  const require = createRequire(app.isPackaged
    ? path.join(process.resourcesPath, "app.asar.unpacked", "native-applications.cjs") : import.meta.url);
  return tools ??= Promise.all([
    import(/* @vite-ignore */ pathToFileURL(require.resolve("app-path")).href) as Promise<typeof import("app-path")>,
    import(/* @vite-ignore */ pathToFileURL(require.resolve("open")).href) as Promise<typeof import("open")>,
  ]).then(([paths, opening]) => ({ appPath: paths.default, open: opening.default, openApp: opening.openApp }));
}
