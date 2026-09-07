import { expect, it, vi } from "vitest";
import { runInNewContext } from "node:vm";
import { evaluate } from "./desktop-smoke-evaluate.mjs";

it.each(["The session has been removed.", "Session example was not found."])(
  "preserves the code independently of wording: %s", async (message) => {
    const call = vi.fn(async (method, params) => {
      if (method === "Runtime.evaluate") {
        return { exceptionDetails: { text: "Uncaught", exception: { objectId: "failure" } } };
      }
      const readError = runInNewContext(`(${params.functionDeclaration})`);
      return { result: { value: readError.call({ code: "resourceNotFound", message }) } };
    });
    await expect(evaluate({ call }, "invoke()"))
      .rejects.toMatchObject({ code: "resourceNotFound", message });
  },
);

it("returns successful evaluation data without inspecting exceptions", async () => {
  const call = vi.fn().mockResolvedValue({ result: { value: { activeTurn: null } } });
  await expect(evaluate({ call }, "invoke()")).resolves.toEqual({ activeTurn: null });
  expect(call).toHaveBeenCalledTimes(1);
});
