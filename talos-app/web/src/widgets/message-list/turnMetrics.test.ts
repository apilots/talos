import { describe, expect, it } from "vitest";

import {
  calculateTurnTokensPerSecond,
  formatMetricNumber,
  formatTokensPerSecond,
} from "./turnMetrics";

describe("turn metrics formatting", () => {
  it("formats token counts compactly without hiding zero values", () => {
    expect(formatMetricNumber(0)).toBe("0");
    expect(formatMetricNumber(978_200)).toBe("978.2k");
    expect(formatMetricNumber(1_250_000)).toBe("1.3m");
  });

  it("calculates a whole-turn average from output tokens and duration", () => {
    expect(calculateTurnTokensPerSecond(362, 11_081)).toBeCloseTo(32.67, 2);
    expect(formatTokensPerSecond(32.67)).toBe("32.67");
    expect(formatTokensPerSecond(362)).toBe("362");
  });

  it("omits throughput when duration is absent or invalid", () => {
    expect(calculateTurnTokensPerSecond(100, null)).toBeNull();
    expect(calculateTurnTokensPerSecond(100, 0)).toBeNull();
  });
});
