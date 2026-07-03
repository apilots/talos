// @vitest-environment jsdom

import { describe, expect, it } from "vitest";

import { resolveServerBaseUrl } from "./store";

describe("connection store", () => {
  it("uses the current browser origin as the default server URL", () => {
    expect(resolveServerBaseUrl()).toBe(window.location.origin);
  });
});
