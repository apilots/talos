import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { resolveAppServerCommand } from "./app-server-binary";

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("resolveAppServerCommand", () => {
  it("uses explicit environment command in development runtime", () => {
    expect(
      resolveAppServerCommand({
        env: { TALOS_APP_SERVER_BIN: "/tmp/talos-dev" },
        isPackaged: false,
        platform: "darwin",
        resourcesPath: "/unused",
      }),
    ).toBe("/tmp/talos-dev");
  });

  it("requires packaged resources in production runtime", () => {
    const resourcesPath = createResourcesWithBinary("talos");

    expect(
      resolveAppServerCommand({
        env: { TALOS_APP_SERVER_BIN: "/tmp/ignored" },
        isPackaged: true,
        platform: "darwin",
        resourcesPath,
      }),
    ).toBe(path.join(resourcesPath, "app-server", "talos"));
  });

  it("fails fast when the packaged app-server binary is missing", () => {
    const resourcesPath = createTempDir();

    expect(() =>
      resolveAppServerCommand({
        env: {},
        isPackaged: true,
        platform: "linux",
        resourcesPath,
      }),
    ).toThrow(`Packaged app-server binary is missing: ${path.join(resourcesPath, "app-server", "talos")}`);
  });
});

function createResourcesWithBinary(name: string): string {
  const dir = createTempDir();
  const appServerDir = path.join(dir, "app-server");
  mkdirSync(appServerDir, { recursive: true });
  writeFileSync(path.join(appServerDir, name), "");
  return dir;
}

function createTempDir(): string {
  const dir = mkdtempSync(path.join(os.tmpdir(), "talos-desktop-test-"));
  tempDirs.push(dir);
  return dir;
}
