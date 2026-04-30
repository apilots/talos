import { describe, expect, it } from "vitest";

import { AppServerProtocolError } from "./stdio";

describe("AppServerProtocolError", () => {
  it("preserves protocol error fields", () => {
    const error = new AppServerProtocolError({
      code: -32002,
      message: "missing session",
      data: { type: "resource", kind: "not_found", resource: "session missing" },
    });

    expect(error.name).toBe("AppServerProtocolError");
    expect(error.message).toBe("missing session");
    expect(error.code).toBe(-32002);
    expect(error.data).toEqual({
      type: "resource",
      kind: "not_found",
      resource: "session missing",
    });
  });
});
