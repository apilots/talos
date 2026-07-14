import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const appRoot = join(scriptDir, "..");
const webSrcRoot = join(appRoot, "web", "src");
const featuresRoot = join(webSrcRoot, "features");

function toPosixPath(filePath) {
  return filePath.split(sep).join("/");
}

function collectFeatureDirectories() {
  return readdirSync(featuresRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
}

function readJsonFile(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function collectPackageJsonPaths() {
  const packagePaths = [
    join(appRoot, "package.json"),
    join(appRoot, "desktop", "package.json"),
    join(appRoot, "web", "package.json"),
    join(appRoot, "ts-sdk", "package.json"),
  ];
  const sdkPackagesRoot = join(appRoot, "ts-sdk", "packages");
  for (const entry of readdirSync(sdkPackagesRoot, { withFileTypes: true })) {
    const packagePath = join(sdkPackagesRoot, entry.name, "package.json");
    if (entry.isDirectory() && existsSync(packagePath)) {
      packagePaths.push(packagePath);
    }
  }
  return packagePaths.sort();
}

function validatePackageMetadata(errors) {
  const rootPackagePath = join(appRoot, "package.json");
  const rootPackage = readJsonFile(rootPackagePath);
  const expectedVersion = rootPackage.version;
  if (!expectedVersion) {
    errors.push("Root package.json is missing version");
    return;
  }

  for (const packagePath of collectPackageJsonPaths()) {
    const label = toPosixPath(relative(appRoot, packagePath));
    if (!existsSync(packagePath)) {
      errors.push(`Missing package metadata file: ${label}`);
      continue;
    }

    const packageJson = readJsonFile(packagePath);
    if (!packageJson.name) {
      errors.push(`Package metadata missing name: ${label}`);
    }
    if (packageJson.version !== expectedVersion) {
      errors.push(
        `Package version mismatch: ${label} has ${packageJson.version ?? "<missing>"}, expected ${expectedVersion}`,
      );
    }
    if (packageJson.private !== true) {
      errors.push(`Package must be private unless explicitly publishable: ${label}`);
    }
  }
}

function validateFeatureBarrels(errors) {
  for (const featureName of collectFeatureDirectories()) {
    const barrelPath = join(featuresRoot, featureName, "index.ts");
    if (!existsSync(barrelPath)) {
      errors.push(`Missing feature barrel: web/src/features/${featureName}/index.ts`);
    }
  }
}

function main() {
  const errors = [];

  validatePackageMetadata(errors);
  validateFeatureBarrels(errors);

  if (errors.length > 0) {
    console.error("repo checks failed:");
    for (const error of errors) {
      console.error(`- ${error}`);
    }
    process.exit(1);
  }

  console.log("repo checks passed");
}

main();
