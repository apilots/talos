// @vitest-environment jsdom

import { describe, expect, it } from "vitest";

import { resolveServerBaseUrl } from "@/shared/api/server-base-url";

describe("connection store", () => {
  it("uses the current browser origin as the default server URL", () => {
    expect(resolveServerBaseUrl()).toBe(window.location.origin);
  });
});
