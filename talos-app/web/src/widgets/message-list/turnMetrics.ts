export function formatMetricNumber(value: number) {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}m`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}k`;
  }
  return String(value);
}

/** Average output throughput over the whole Turn, not model decode speed. */
export function calculateTurnTokensPerSecond(outputTokens: number, durationMs: number | null) {
  if (durationMs === null || durationMs <= 0) {
    return null;
  }
  return outputTokens / (durationMs / 1000);
}

export function formatTokensPerSecond(value: number) {
  return String(Number(value.toFixed(2)));
}
