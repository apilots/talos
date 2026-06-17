import { existsSync, lstatSync, readdirSync, readFileSync } from "node:fs";
import { dirname, extname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const webRoot = join(scriptDir, "..", "web");
const webSrcRoot = join(webRoot, "src");
const featuresRoot = join(webSrcRoot, "features");
const ignoredDirectories = new Set(["dist", "node_modules", ".git"]);
const maxEffectiveLinesPerFile = 1000;
const maxEffectiveLinesPerTestFile = 300;
const legacyLineBaselines = new Map([
  ["src/features/settings/components/models/SettingsModelsEditor.tsx", 417],
  ["src/features/chat/components/ChatComposer.tsx", 574],
  ["src/features/chat/__tests__/streaming.test.ts", 637],
  ["src/features/chat/store.ts", 516],
  ["src/features/settings/components/SettingsAgentWorkspaceSection.tsx", 374],
  ["src/shared/i18n/resources.ts", 1037],
  ["src/widgets/message-list/MessageList.test.tsx", 442],
]);

const lineRules = [
  { pattern: /\/pages\/.+\.tsx$/, limit: 300, label: "pages" },
  { pattern: /\/components\/.+\.tsx$/, limit: 200, label: "components" },
  { pattern: /\/hooks\/.+\.tsx?$/, limit: 300, label: "hooks" },
  { pattern: /\/store\.ts$|\/store\/[^/]+\.tsx?$/, limit: 300, label: "stores" },
];

function toPosixPath(filePath) {
  return filePath.split(sep).join("/");
}

function countNonEmptyLines(filePath) {
  return readFileSync(filePath, "utf8").split(/\r?\n/).filter((line) => line.trim().length > 0).length;
}

function isTestFile(relativePath) {
  return /\.(test|spec)\.tsx?$/.test(relativePath);
}

function collectFiles(dirPath, files = []) {
  for (const entry of readdirSync(dirPath, { withFileTypes: true })) {
    if (ignoredDirectories.has(entry.name)) {
      continue;
    }

    const fullPath = join(dirPath, entry.name);
    const stats = lstatSync(fullPath);
    if (stats.isSymbolicLink()) {
      continue;
    }

    if (entry.isDirectory()) {
      collectFiles(fullPath, files);
      continue;
    }

    files.push(fullPath);
  }

  return files;
}

function collectFeatureDirectories() {
  return readdirSync(featuresRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
}

function validateFeatureBarrels(errors) {
  for (const featureName of collectFeatureDirectories()) {
    const barrelPath = join(featuresRoot, featureName, "index.ts");
    if (!existsSync(barrelPath)) {
      errors.push(`Missing feature barrel: web/src/features/${featureName}/index.ts`);
    }
  }
}

function validateLineRules(errors) {
  const sourceFiles = collectFiles(webSrcRoot).filter((filePath) => {
    const extension = extname(filePath);
    return extension === ".ts" || extension === ".tsx";
  });

  for (const filePath of sourceFiles) {
    const relativePath = toPosixPath(relative(webRoot, filePath));
    const normalizedPath = `/${toPosixPath(relative(webSrcRoot, filePath))}`;
    const lineCount = countNonEmptyLines(filePath);
    const legacyBaseline = legacyLineBaselines.get(relativePath);

    if (legacyBaseline !== undefined) {
      if (lineCount > legacyBaseline) {
        errors.push(
          `File exceeds legacy line baseline (${legacyBaseline}): ${relativePath} (${lineCount})`,
        );
      }
      continue;
    }

    if (isTestFile(relativePath)) {
      if (lineCount > maxEffectiveLinesPerTestFile) {
        errors.push(
          `File exceeds test line limit (${maxEffectiveLinesPerTestFile}): ${relativePath} (${lineCount})`,
        );
      }
      continue;
    }

    if (lineCount > maxEffectiveLinesPerFile) {
      errors.push(
        `File exceeds effective line limit (${maxEffectiveLinesPerFile}): ${relativePath} (${lineCount})`,
      );
    }

    for (const rule of lineRules) {
      if (rule.pattern.test(normalizedPath) && lineCount > rule.limit) {
        errors.push(
          `File exceeds ${rule.label} line limit (${rule.limit}): ${relativePath} (${lineCount})`,
        );
        break;
      }
    }
  }
}

function main() {
  const errors = [];

  validateFeatureBarrels(errors);
  validateLineRules(errors);

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
