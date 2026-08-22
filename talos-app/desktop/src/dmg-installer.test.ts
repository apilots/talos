import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

interface DmgContent {
  x: number;
  y: number;
  type: "file" | "link";
  path?: string;
}

interface DmgConfiguration {
  background: string;
  contents: DmgContent[];
  iconSize: number;
  title: string;
  window: {
    width: number;
    height: number;
  };
}

interface DesktopBuilderConfiguration {
  dmg: DmgConfiguration;
}

const require = createRequire(import.meta.url);
const dirname = path.dirname(fileURLToPath(import.meta.url));
const builderConfigPath = path.resolve(dirname, "../electron-builder.config.cjs");
const builderConfig = require(builderConfigPath) as DesktopBuilderConfiguration;

describe("macOS DMG installer", () => {
  it("provides standard and Retina backgrounds matching the Finder window", () => {
    const { background, window } = builderConfig.dmg;
    const retinaBackground = background.replace(/\.png$/, "@2x.png");

    expect(readPngSize(background)).toEqual(window);
    expect(readPngSize(retinaBackground)).toEqual({
      width: window.width * 2,
      height: window.height * 2,
    });
  });

  it("exposes one clear drag path from Talos to Applications", () => {
    expect(builderConfig.dmg).toMatchObject({
      contents: [
        { x: 140, y: 205, type: "file" },
        { x: 400, y: 205, type: "link", path: "/Applications" },
      ],
      iconSize: 108,
      title: "Talos",
      window: { width: 540, height: 380 },
    });
  });
});

function readPngSize(filePath: string): { width: number; height: number } {
  const header = readFileSync(filePath).subarray(0, 24);
  const pngSignature = header.subarray(0, 8).toString("hex");

  if (pngSignature !== "89504e470d0a1a0a" || header.toString("ascii", 12, 16) !== "IHDR") {
    throw new Error(`DMG background is not a valid PNG: ${filePath}`);
  }

  return {
    width: header.readUInt32BE(16),
    height: header.readUInt32BE(20),
  };
}
