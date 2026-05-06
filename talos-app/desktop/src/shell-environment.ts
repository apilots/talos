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
  execLoginShell?: (shell: string) => Promise<NodeJS.ProcessEnv>;
}

export async function resolveAppServerEnvironment(
  options: ShellEnvironmentOptions,
): Promise<NodeJS.ProcessEnv> {
  const baseEnv = { ...options.baseEnv };
  if (!shouldLoadLoginShellEnvironment(options)) {
    return baseEnv;
  }

  const shellEnv = await loadLoginShellEnvironment({
    baseEnv,
    shell: options.shell,
    placeholderNames: readConfigPlaceholderNames(resolveConfigPath(baseEnv, options.configPath)),
    execLoginShell: options.execLoginShell,
  });
  return {
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
