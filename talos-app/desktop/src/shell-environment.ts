import { execFile } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

export interface ShellEnvironmentOptions {
  baseEnv: NodeJS.ProcessEnv;
  isPackaged: boolean;
  platform: NodeJS.Platform;
  shell?: string;
  configPath?: string;
  envPath?: string;
  execLoginShell?: (shell: string) => Promise<NodeJS.ProcessEnv>;
}

export async function resolveAppServerEnvironment(
  options: ShellEnvironmentOptions,
): Promise<NodeJS.ProcessEnv> {
  const baseEnv = { ...options.baseEnv };
  if (!shouldLoadLoginShellEnvironment(options)) {
    return baseEnv;
  }
  const talosEnv = readTalosEnvFile(resolveTalosEnvPath(baseEnv, options.envPath));
  const configEnv = {
    ...talosEnv,
    ...baseEnv,
  };

  const shellEnv = await loadLoginShellEnvironment({
    baseEnv: configEnv,
    shell: options.shell,
    placeholderNames: readConfigPlaceholderNames(resolveConfigPath(configEnv, options.configPath)),
    execLoginShell: options.execLoginShell,
  });
  return {
    ...talosEnv,
    ...shellEnv,
    ...baseEnv,
  };
}

function shouldLoadLoginShellEnvironment(options: ShellEnvironmentOptions): boolean {
  return options.isPackaged && options.platform === "darwin";
}

interface LoadLoginShellEnvironmentOptions {
  baseEnv: NodeJS.ProcessEnv;
  shell?: string;
  placeholderNames: string[];
  execLoginShell?: (shell: string) => Promise<NodeJS.ProcessEnv>;
}

async function loadLoginShellEnvironment(
  options: LoadLoginShellEnvironmentOptions,
): Promise<NodeJS.ProcessEnv> {
  const shell = options.shell ?? options.baseEnv.SHELL ?? "/bin/zsh";
  try {
    return options.execLoginShell
      ? await options.execLoginShell(shell)
      : await execLoginShell(shell, options.placeholderNames);
  } catch (error) {
    throw new Error(`Failed to load login shell environment from ${shell}: ${errorMessage(error)}`);
  }
}

async function execLoginShell(
  shell: string,
  placeholderNames: string[],
): Promise<NodeJS.ProcessEnv> {
  const exportScript = placeholderNames.map((name) => `export ${name}`).join("; ");
  const script = exportScript ? `${exportScript}; env` : "env";
  const { stdout } = await execFileAsync(shell, ["-ilc", script], {
    timeout: 5_000,
    maxBuffer: 1024 * 1024,
  });
  return parseEnvironment(stdout);
}

function resolveConfigPath(env: NodeJS.ProcessEnv, configPath?: string): string {
  if (configPath) {
    return configPath;
  }
  const talosHome = env.TALOS_HOME?.trim() || path.join(os.homedir(), ".talos");
  return path.join(talosHome, "conf", "talos.yaml");
}

function resolveTalosEnvPath(env: NodeJS.ProcessEnv, envPath?: string): string {
  if (envPath) {
    return envPath;
  }
  const talosHome = env.TALOS_HOME?.trim() || path.join(os.homedir(), ".talos");
  return path.join(talosHome, ".env");
}

export function readTalosEnvFile(envPath: string): NodeJS.ProcessEnv {
  if (!existsSync(envPath)) {
    return {};
  }
  return parseDotEnv(readFileSync(envPath, "utf8"));
}

export function parseDotEnv(content: string): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) {
      continue;
    }
    const normalized = line.startsWith("export ") ? line.slice("export ".length).trim() : line;
    const index = normalized.indexOf("=");
    if (index <= 0) {
      continue;
    }
    const key = normalized.slice(0, index).trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
      continue;
    }
    env[key] = unquoteEnvValue(normalized.slice(index + 1).trim());
  }
  return env;
}

function unquoteEnvValue(value: string): string {
  if (
    (value.startsWith("\"") && value.endsWith("\"")) ||
    (value.startsWith("'") && value.endsWith("'"))
  ) {
    return value.slice(1, -1);
  }
  const commentIndex = value.indexOf(" #");
  return commentIndex >= 0 ? value.slice(0, commentIndex).trimEnd() : value;
}

export function readConfigPlaceholderNames(configPath: string): string[] {
  if (!existsSync(configPath)) {
    return [];
  }
  const content = readFileSync(configPath, "utf8");
  const names = new Set<string>();
  for (const match of content.matchAll(/\$\{([A-Za-z_][A-Za-z0-9_]*)\}/g)) {
    names.add(match[1]);
  }
  return [...names].sort();
}

export function parseEnvironment(output: string): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const rawLine of output.split(/\r?\n/)) {
    const index = rawLine.indexOf("=");
    if (index <= 0) {
      continue;
    }
    const key = rawLine.slice(0, index);
    env[key] = rawLine.slice(index + 1);
  }
  return env;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
