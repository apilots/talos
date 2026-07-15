// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  clearWebAuthToken,
  readWebAuthToken,
  storeWebAuthToken,
} from "./web-auth-session";

describe("web auth session", () => {
  beforeEach(() => {
    window.sessionStorage.clear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("stores the token only for the current browser tab", () => {
    storeWebAuthToken("web-token");

    expect(readWebAuthToken()).toBe("web-token");
  });

  it("clears the token", () => {
    storeWebAuthToken("web-token");

    clearWebAuthToken();

    expect(readWebAuthToken()).toBeNull();
  });

  it("keeps authentication usable when browser session storage is unavailable", () => {
    const unavailableStorage = {
      getItem: () => { throw new DOMException("Storage is unavailable.", "SecurityError"); },
      setItem: () => { throw new DOMException("Storage is unavailable.", "SecurityError"); },
      removeItem: () => { throw new DOMException("Storage is unavailable.", "SecurityError"); },
      clear: vi.fn(),
      key: vi.fn(),
      length: 0,
    } satisfies Storage;
    vi.stubGlobal("sessionStorage", unavailableStorage);

    expect(() => storeWebAuthToken("web-token")).not.toThrow();
    expect(readWebAuthToken()).toBeNull();
    expect(() => clearWebAuthToken()).not.toThrow();
  });
});
