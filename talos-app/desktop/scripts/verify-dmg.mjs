import { execFileSync } from "node:child_process";
import {
  lstatSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  rmSync,
  statSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "darwin") {
  throw new Error("DMG verification requires macOS.");
}

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const desktopDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(desktopDir, "../..");
const productDir = process.env.TALOS_DESKTOP_PACKAGE_DIR ??
  path.join(repoRoot, "dist/products/dev/desktop-mac");
const dmgPath = process.argv[2] ?? findDmg(productDir);
const mountDir = mkdtempSync(path.join(os.tmpdir(), "talos-dmg-"));
let mounted = false;

try {
  run("hdiutil", ["attach", "-nobrowse", "-readonly", "-mountpoint", mountDir, dmgPath]);
  mounted = true;

  assertDirectory(path.join(mountDir, "Talos.app"));
  assertApplicationsLink(path.join(mountDir, "Applications"));
  assertFinderMetadata(path.join(mountDir, ".DS_Store"));
  assertBackground(path.join(mountDir, ".background.tiff"));

  console.log(`Verified packaged DMG Finder layout: ${dmgPath}`);
} finally {
  if (mounted) {
    run("hdiutil", ["detach", mountDir]);
  }
  rmSync(mountDir, { recursive: true, force: true });
}

function findDmg(directory) {
  const matches = readdirSync(directory)
    .filter((name) => name.endsWith(".dmg"))
    .sort();
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one DMG in ${directory}, found ${matches.length}.`);
  }
  return path.join(directory, matches[0]);
}

function assertDirectory(directory) {
  if (!statSync(directory).isDirectory()) {
    throw new Error(`Missing packaged application: ${directory}`);
  }
}

function assertApplicationsLink(linkPath) {
  if (!lstatSync(linkPath).isSymbolicLink() || readlinkSync(linkPath) !== "/Applications") {
    throw new Error(`Invalid Applications link: ${linkPath}`);
  }
}

function assertFinderMetadata(dsStorePath) {
  const metadata = readFileSync(dsStorePath);
  if (metadata.length === 0) {
    throw new Error(`Finder metadata is empty: ${dsStorePath}`);
  }

  for (const record of ["bwsp", "icvp", "Iloc"]) {
    if (!metadata.includes(Buffer.from(record))) {
      throw new Error(`Finder metadata does not contain the ${record} layout record.`);
    }
  }

  for (const item of ["Talos.app", "Applications"]) {
    if (!metadata.includes(utf16be(item))) {
      throw new Error(`Finder metadata does not position ${item}.`);
    }
  }
}

function assertBackground(backgroundPath) {
  const sourcePath = path.join(repoRoot, "build/desktop/dmg/background.png");
  const sourceSize = readPngSize(sourcePath);
  const retinaSize = readPngSize(sourcePath.replace(/\.png$/, "@2x.png"));
  const mountedSizes = readTiffSizes(backgroundPath);

  if (mountedSizes.length !== 2) {
    throw new Error(`Expected two mounted DMG background representations, found ${mountedSizes.length}.`);
  }
  assertEqual(mountedSizes[0], sourceSize, "mounted DMG background");
  assertEqual(mountedSizes[1], retinaSize, "mounted Retina DMG background");
  assertEqual(retinaSize, {
    width: sourceSize.width * 2,
    height: sourceSize.height * 2,
  }, "Retina DMG background");
}

function readTiffSizes(filePath) {
  const output = execFileSync("tiffutil", ["-info", filePath], {
    encoding: "utf8",
  });
  const sizes = [...output.matchAll(/Image Width: (\d+) Image Length: (\d+)/g)].map((match) => ({
    width: Number(match[1]),
    height: Number(match[2]),
  }));
  if (sizes.length === 0) {
    throw new Error(`Unable to read TIFF image dimensions: ${filePath}`);
  }
  return sizes;
}

function readPngSize(filePath) {
  const header = readFileSync(filePath).subarray(0, 24);
  if (
    header.subarray(0, 8).toString("hex") !== "89504e470d0a1a0a" ||
    header.toString("ascii", 12, 16) !== "IHDR"
  ) {
    throw new Error(`DMG background is not a valid PNG: ${filePath}`);
  }
  return {
    width: header.readUInt32BE(16),
    height: header.readUInt32BE(20),
  };
}

function utf16be(value) {
  return Buffer.from(value, "utf16le").swap16();
}

function assertEqual(actual, expected, label) {
  if (actual.width !== expected.width || actual.height !== expected.height) {
    throw new Error(
      `Invalid ${label} dimensions: ${actual.width}x${actual.height}; expected ${expected.width}x${expected.height}.`,
    );
  }
}

function run(command, args) {
  execFileSync(command, args, { stdio: "inherit" });
}
