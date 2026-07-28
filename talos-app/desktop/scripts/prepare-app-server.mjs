import { spawnSync } from "node:child_process";
import { chmodSync, cpSync, copyFileSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const desktopDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(desktopDir, "../..");
const rustDir = path.join(repoRoot, "talos-rs");
const confDir = path.join(repoRoot, "conf");
const resourcesDir = path.join(desktopDir, "resources", "app-server");
const resourcesConfDir = path.join(desktopDir, "resources", "conf");
const profile = process.env.TALOS_DESKTOP_APP_SERVER_PROFILE ?? process.env.PROFILE ?? "dev";
const binaryName = process.platform === "win32" ? "app-server.exe" : "app-server";
const sourceBinary =
  process.env.TALOS_DESKTOP_APP_SERVER_BIN ??
  process.env.TALOS_APP_SERVER_BIN ??
  buildAppServer();

rmSync(resourcesDir, { recursive: true, force: true });
rmSync(resourcesConfDir, { recursive: true, force: true });
mkdirSync(resourcesDir, { recursive: true });
mkdirSync(resourcesConfDir, { recursive: true });

const destination = path.join(resourcesDir, binaryName);
copyFileSync(sourceBinary, destination);
chmodSync(destination, 0o755);
console.log(`Prepared desktop app-server binary: ${destination}`);

const configTemplateSource =
  process.env.TALOS_DESKTOP_CONFIG_TEMPLATE ??
  path.join(confDir, profile === "release" ? "talos-prod.yaml" : "talos-dev.yaml");
const configTemplateDestination = path.join(resourcesConfDir, "talos.yaml");
copyFileSync(configTemplateSource, configTemplateDestination);
console.log(`Prepared desktop config template: ${configTemplateDestination}`);

const migrationSource = path.join(confDir, "migrations");
const migrationDestination = path.join(resourcesConfDir, "migrations");
cpSync(migrationSource, migrationDestination, { recursive: true });
console.log(`Prepared desktop migrations: ${migrationDestination}`);

function buildAppServer() {
  const args = [
    "build",
    "--manifest-path",
    path.join(rustDir, "Cargo.toml"),
    "-p",
    "app-server",
    "--bin",
    "app-server",
  ];
  if (profile === "release") {
    args.push("--release");
  }
  run("cargo", args, repoRoot);
  return path.join(rustDir, "target", profile === "release" ? "release" : "debug", binaryName);
}

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    env: process.env,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with status ${result.status ?? "null"}`);
  }
}
