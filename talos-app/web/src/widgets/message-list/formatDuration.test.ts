import { describe, expect, it } from "vitest";

import { formatDuration } from "@/shared/lib/formatDuration";

describe("formatDuration", () => {
  it("formats shared timeline and tool durations", () => {
    expect(formatDuration(42)).toBe("42ms");
    expect(formatDuration(999)).toBe("999ms");
    expect(formatDuration(1500)).toBe("1.5s");
    expect(formatDuration(23_400)).toBe("23s");
    expect(formatDuration(77_000)).toBe("1m17s");
  });
});

describe("localized duration", () => {
  it.each([
    [0, "0 ms", "0毫秒"],
    [999, "999 ms", "999毫秒"],
    [1_500, "1.5 sec", "1.5秒"],
    [60_000, "1 min", "1分钟"],
    [90_000, "1.5 min", "1.5分钟"],
    [7_200_000, "2 hr", "2小时"],
  ] as const)("formats %d milliseconds in the appropriate localized unit", (milliseconds, en, zh) => {
    expect(formatDuration(milliseconds, "en")).toBe(en);
    expect(formatDuration(milliseconds, "zh-CN")).toBe(zh);
  });
});
