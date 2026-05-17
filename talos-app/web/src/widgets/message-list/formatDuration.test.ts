import { describe, expect, it } from "vitest";

import { formatDuration } from "./formatDuration";

describe("formatDuration", () => {
  it("formats shared timeline and tool durations", () => {
    expect(formatDuration(42)).toBe("42ms");
    expect(formatDuration(999)).toBe("999ms");
    expect(formatDuration(1500)).toBe("1.5s");
    expect(formatDuration(23_400)).toBe("23s");
    expect(formatDuration(77_000)).toBe("1m17s");
  });
});
