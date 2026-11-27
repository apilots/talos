import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, copyFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const cli = new URL("../node_modules/dependency-cruiser/bin/dependency-cruiser.mjs", import.meta.url).pathname;

test("layer policy resolves relative imports and aliases while accepting feature barrels", () => {
  const root = mkdtempSync(join(tmpdir(), "talos-architecture-"));
  try {
    for (const directory of ["features/example", "shared/lib", "shared/i18n", "shared/app-icons", "shared/new-capability", "widgets", "pages"]) mkdirSync(join(root, "web/src", directory), { recursive: true });
    copyFileSync(new URL("../.dependency-cruiser.cjs", import.meta.url), join(root, ".dependency-cruiser.cjs"));
    writeFileSync(join(root, "web/tsconfig.json"), JSON.stringify({ compilerOptions: { baseUrl: ".", paths: { "@/*": ["src/*"] } } }));
    writeFileSync(join(root, "web/src/features/example/private.ts"), "export const value = 1;\n");
    writeFileSync(join(root, "web/src/features/example/index.ts"), 'export { value } from "./private";\n');
    writeFileSync(join(root, "web/src/pages/page.ts"), 'export { value } from "@/features/example";\n');
    const args = [cli, "--config", ".dependency-cruiser.cjs", "web/src"];
    execFileSync(process.execPath, args, { cwd: root, stdio: "pipe" });
    for (const [file, source, rule] of [
      ["shared/lib/helper.ts", "../../features/example/private", "shared-does-not-import-features"],
      ["shared/i18n/translations.ts", "@/features/example/private", "shared-does-not-import-features"],
      ["shared/app-icons/icon.ts", "@/features/example/private", "shared-does-not-import-features"],
      ["shared/new-capability/helper.ts", "@/features/example/private", "shared-does-not-import-features"],
      ["widgets/widget.ts", "@/features/example/private", "widgets-receive-feature-data"],
      ["pages/page.ts", "../features/example/private", "composition-uses-feature-barrels"],
    ]) {
      writeFileSync(join(root, "web/src", file), `export { value } from "${source}";\n`);
      const result = spawnSync(process.execPath, args, { cwd: root, encoding: "utf8" });
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(result.stdout + result.stderr, new RegExp(rule));
      rmSync(join(root, "web/src", file));
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
