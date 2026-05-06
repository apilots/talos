import path from "node:path";

export interface RendererEntryOptions {
  appPath: string;
  dirname: string;
  isPackaged: boolean;
}

export function resolveRendererEntryPath(options: RendererEntryOptions): string {
  if (options.isPackaged) {
    return path.join(options.appPath, "web/dist/index.html");
  }
  return path.resolve(options.dirname, "../../web/dist/index.html");
}
