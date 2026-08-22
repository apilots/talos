import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

interface DmgConfiguration {
  background: string;
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
