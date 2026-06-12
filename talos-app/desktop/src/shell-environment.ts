import path from "node:path";

export interface ShellEnvironmentOptions {
  baseEnv: NodeJS.ProcessEnv;
  packagedResourcesPath?: string | null;
}

const PASSTHROUGH_ENV_KEYS = new Set(["HOME", "PATH", "SHELL", "TMPDIR", "USER"]);

export function resolveAppServerEnvironment(options: ShellEnvironmentOptions): NodeJS.ProcessEnv {
  const env = filterAppServerEnvironment(options.baseEnv);
  if (options.packagedResourcesPath) {
    env.TALOS_RESOURCE_DIR ??= options.packagedResourcesPath;
    env.TALOS_CONFIG ??= path.join(options.packagedResourcesPath, "conf", "talos.yaml");
  }
  return env;
}

export function filterAppServerEnvironment(baseEnv: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const [key, value] of Object.entries(baseEnv)) {
    if (value === undefined) {
      continue;
    }
    if (key.startsWith("TALOS_") || PASSTHROUGH_ENV_KEYS.has(key)) {
      env[key] = value;
    }
  }
  return env;
}
