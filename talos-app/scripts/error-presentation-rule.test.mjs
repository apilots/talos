import assert from "node:assert/strict";
import test from "node:test";
import { createRequire } from "node:module";
const { ESLint } = createRequire(new URL("../web/package.json", import.meta.url))("eslint");

test("typed errors require the presenter but ordinary domain messages remain legal", async () => {
  const lint = new ESLint({ cwd: new URL("..", import.meta.url).pathname });
  const [result] = await lint.lintText(`
    import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
    import type { AppProtocol } from "@talos/sdk-core";
    export function invalid(error: Error) { return error.message; }
    export function invalidApi(error: AppProtocol.ApiError) { return error.message; }
    export function invalidDestructure(error: Error) { const { message } = error; return message; }
    export function invalidParameter({ message }: AppProtocol.ApiError) { return message; }
    export function invalidRename(error: AppProtocol.ApiError) { const { message: reason } = error; return reason; }
    export function invalidNested({ error: { message } }: { error: AppProtocol.ApiError }) { return message; }
    declare function setError(value: string): void;
    export function invalidStored(error: Error) { setError(userVisibleErrorMessage(error, "Request failed.")); }
    export function valid(error: Error) { return userVisibleErrorMessage(error, "Request failed."); }
    export function domain(item: { message: string }) { return item.message; }
    export function domainDestructure({ message }: { message: string }) { return message; }
    export function errorCode({ code }: AppProtocol.ApiError) { return code; }
  `, { filePath: "web/src/shared/i18n/errorResources.ts" });
  assert.deepEqual(result.messages.map(({ ruleId }) => ruleId), [
    "talos/error-presentation", "talos/error-presentation",
    "talos/error-presentation", "talos/error-presentation",
    "talos/error-presentation", "talos/error-presentation",
    "talos/error-presentation",
  ]);
});
