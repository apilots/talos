import { beforeEach, describe, expect, it, vi } from "vitest";

import { formatRelativeTime } from "./time";

describe("time helpers", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-17T12:00:00Z"));
  });

  it("formats recent timestamps as relative labels", () => {
    expect(formatRelativeTime(Date.now() - 20_000)).toBe("Just now");
    expect(formatRelativeTime(Date.now() - 10 * 60_000)).toBe("10m ago");
    expect(formatRelativeTime(Date.now() - 2 * 60 * 60_000)).toBe("2h ago");
  });
});
