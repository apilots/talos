export function formatDuration(durationMs: number, locale?: string) {
  if (locale) {
    const milliseconds = Math.max(0, Math.round(durationMs));
    const duration = milliseconds >= 3_600_000 ? { value: milliseconds / 3_600_000, unit: "hour" }
      : milliseconds >= 60_000 ? { value: milliseconds / 60_000, unit: "minute" }
        : milliseconds >= 1_000 ? { value: milliseconds / 1_000, unit: "second" }
          : { value: milliseconds, unit: "millisecond" };
    return new Intl.NumberFormat(locale, {
      style: "unit", unit: duration.unit, unitDisplay: "short",
      maximumFractionDigits: duration.unit === "millisecond" ? 0 : 1,
    }).format(duration.value);
  }
  if (durationMs < 1000) {
    return `${Math.max(0, Math.round(durationMs))}ms`;
  }
  if (durationMs < 10_000) {
    return `${(durationMs / 1000).toFixed(1).replace(/\.0$/, "")}s`;
  }
  const totalSeconds = Math.round(durationMs / 1000);
  if (totalSeconds < 60) {
    return `${totalSeconds}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m${seconds.toString().padStart(2, "0")}s`;
}
