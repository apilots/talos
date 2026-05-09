export interface ShellEnvironmentOptions {
  baseEnv: NodeJS.ProcessEnv;
}

const PASSTHROUGH_ENV_KEYS = new Set(["HOME", "PATH", "SHELL", "TMPDIR", "USER"]);

export function resolveAppServerEnvironment(options: ShellEnvironmentOptions): NodeJS.ProcessEnv {
  return filterAppServerEnvironment(options.baseEnv);
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
