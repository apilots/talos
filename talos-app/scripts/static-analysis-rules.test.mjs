import assert from "node:assert/strict";
import test from "node:test";
import { createRequire } from "node:module";

const { ESLint } = createRequire(new URL("../web/package.json", import.meta.url))("eslint");
const lint = new ESLint({ cwd: new URL("..", import.meta.url).pathname });

const cases = [
  {
    rule: "vitest/unbound-method",
    filePath: "web/src/features/computers/ComputerDesktop.test.tsx",
    invalid: 'class Service { run() { return this; } } const service = new Service(); export const run = service.run;',
    valid: 'import { expect } from "vitest"; class Service { run() { return this; } } const service = new Service(); expect(service.run).toHaveBeenCalled(); export const run = service.run.bind(service);',
  },
  {
    rule: "@typescript-eslint/no-unsafe-assignment",
    invalid: 'export const value = JSON.parse("{}");',
    valid: 'export const value: unknown = JSON.parse("{}");',
  },
  {
    rule: "@typescript-eslint/no-unsafe-member-access",
    invalid: 'export const value = JSON.parse("{}").name;',
    valid: 'const value: unknown = JSON.parse("{}"); export const name = value && typeof value === "object" && "name" in value ? value.name : null;',
  },
  {
    rule: "react-hooks/exhaustive-deps",
    invalid: 'import { useEffect } from "react"; export function Component({ name }: { name: string }) { useEffect(() => { document.title = name; }, []); return null; }',
    valid: 'import { useEffect } from "react"; export function Component({ name }: { name: string }) { useEffect(() => { document.title = name; }, [name]); return null; }',
  },
  {
    rule: "@tanstack/query/exhaustive-deps",
    invalid: 'import { useQuery } from "@tanstack/react-query"; export function useItem(id: string) { return useQuery({ queryKey: ["item"], queryFn: () => Promise.resolve(id) }); }',
    valid: 'import { useQuery } from "@tanstack/react-query"; export function useItem(id: string) { return useQuery({ queryKey: ["item", id], queryFn: () => Promise.resolve(id) }); }',
  },

  {
    rule: "@typescript-eslint/no-floating-promises",
    invalid: "export function run() { Promise.resolve(); }",
    valid: "export function run() { return Promise.resolve(); }",
  },
  {
    rule: "@typescript-eslint/no-misused-promises",
    invalid: "declare function run(callback: () => void): void; run(async () => {});",
    valid: "declare function run(callback: () => Promise<void>): void; run(async () => {});",
  },
  {
    rule: "@typescript-eslint/await-thenable",
    invalid: "export async function run() { await 123; }",
    valid: "export async function run() { await Promise.resolve(123); }",
  },
  {
    rule: "@typescript-eslint/switch-exhaustiveness-check",
    invalid: 'export function run(value: "ok" | "error") { switch (value) { case "ok": return true; } }',
    valid: 'export function run(value: "ok" | "error") { switch (value) { case "ok": return true; case "error": return false; } }',
  },
  {
    rule: "react-hooks/rules-of-hooks",
    invalid: 'import { useState } from "react"; export function Component({ visible }: { visible: boolean }) { if (visible) useState(0); return null; }',
    valid: 'import { useState } from "react"; export function Component() { useState(0); return null; }',
  },
  {
    rule: "jsx-a11y/alt-text",
    invalid: 'export const Picture = () => <img src="/icon.png" />;',
    valid: 'export const Picture = () => <img src="/icon.png" alt="Application icon" />;',
  },
  {
    rule: "@tanstack/query/stable-query-client",
    invalid: 'import { QueryClient } from "@tanstack/react-query"; export function Component() { const client = new QueryClient(); return client.isFetching(); }',
    valid: 'import { useState } from "react"; import { QueryClient } from "@tanstack/react-query"; export function Component() { const [client] = useState(() => new QueryClient()); return client.isFetching(); }',
  },
  {
    rule: "vitest/no-focused-tests",
    filePath: "web/src/features/computers/ComputerDesktop.test.tsx",
    invalid: 'import { it, expect } from "vitest"; it.only("works", () => { expect(1).toBe(1); });',
    valid: 'import { it, expect } from "vitest"; it("works", () => { expect(1).toBe(1); });',
  },
  {
    rule: "testing-library/await-async-queries",
    filePath: "web/src/features/computers/ComputerDesktop.test.tsx",
    invalid: 'import { screen } from "@testing-library/react"; export function run() { screen.findByRole("button"); }',
    valid: 'import { screen } from "@testing-library/react"; export function run() { return screen.findByRole("button"); }',
  },
];

for (const { rule, invalid, valid, filePath = "web/src/shared/ui/Button.tsx" } of cases) {
  test(`${rule} rejects an unsafe pattern and accepts its handled form`, async () => {
    const counts = [];
    for (const code of [invalid, valid]) {
      const [result] = await lint.lintText(code, { filePath });
      if (result.fatalErrorCount) throw new Error(JSON.stringify(result.messages));
      counts.push(result.messages.filter(({ ruleId }) => ruleId === rule).length);
    }
    assert.deepEqual(counts, [1, 0]);
  });
}


test("plain JavaScript does not require a TypeScript project", async () => {
  const [result] = await lint.lintText('export const value = JSON.parse("42");', {
    filePath: "web/src/static-analysis-probe.mjs",
  });
  assert.equal(result.errorCount, 0, JSON.stringify(result.messages));
});
