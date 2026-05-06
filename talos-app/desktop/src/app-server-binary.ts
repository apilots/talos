import { existsSync } from "node:fs";
import path from "node:path";

export interface AppServerBinaryResolutionOptions {
  env: NodeJS.ProcessEnv;
  isPackaged: boolean;
  platform: NodeJS.Platform;
  resourcesPath: string;
}

export function resolveAppServerCommand(options: AppServerBinaryResolutionOptions): string | undefined {
  if (!options.isPackaged) {
    return options.env.TALOS_APP_SERVER_BIN;
  }

  const command = path.join(
    options.resourcesPath,
    "app-server",
    appServerExecutableName(options.platform),
  );
  if (!existsSync(command)) {
    throw new Error(`Packaged app-server binary is missing: ${command}`);
  }
  return command;
}

function appServerExecutableName(platform: NodeJS.Platform): string {
  return platform === "win32" ? "talos.exe" : "talos";
}
