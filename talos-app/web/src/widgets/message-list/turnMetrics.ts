export function formatMetricNumber(value: number) {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}m`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}k`;
  }
  return String(value);
}

export function calculateTurnTokensPerSecond(totalTokens: number, durationMs: number | null) {
  if (durationMs === null || durationMs <= 0) {
    return null;
  }
  return totalTokens / (durationMs / 1000);
}

export function formatTokensPerSecond(value: number) {
  return String(Number(value.toFixed(2)));
}
