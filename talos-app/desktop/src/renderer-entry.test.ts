import path from "node:path";

import { describe, expect, it } from "vitest";

import { resolveRendererEntryPath } from "./renderer-entry";

describe("resolveRendererEntryPath", () => {
  it("loads WebUI from app.asar in packaged runtime", () => {
    expect(
      resolveRendererEntryPath({
        appPath: "/Applications/Talos.app/Contents/Resources/app.asar",
        dirname: "/Applications/Talos.app/Contents/Resources/app.asar/dist",
        isPackaged: true,
      }),
    ).toBe(path.join("/Applications/Talos.app/Contents/Resources/app.asar", "web/dist/index.html"));
  });

  it("loads WebUI from the workspace build in development runtime", () => {
    expect(
      resolveRendererEntryPath({
        appPath: "/repo/talos-app/desktop",
        dirname: "/repo/talos-app/desktop/dist",
        isPackaged: false,
      }),
    ).toBe(path.resolve("/repo/talos-app/desktop/dist", "../../web/dist/index.html"));
  });
});
