export interface RequestParameterRow {
  key: string;
  value: string;
}

const MANAGED_KEYS = new Set([
  "model", "messages", "input", "system", "tools", "tool_choice", "parallel_tool_calls",
  "stream", "stream_options", "api_key", "authorization", "reasoning_replay",
]);

export function parseRequestParameters(rows: RequestParameterRow[]): {
  value: Record<string, unknown>;
  error?: "missingKey" | "duplicate" | "reserved" | "invalidJson";
} {
  const value: Record<string, unknown> = {};
  Object.setPrototypeOf(value, null);
  for (const row of rows) {
    const key = row.key.trim();
    if (!key && !row.value.trim()) continue;
    if (!key) return { value, error: "missingKey" };
    if (Object.hasOwn(value, key)) return { value, error: "duplicate" };
    if (MANAGED_KEYS.has(key)) return { value, error: "reserved" };
    try {
      const parsed: unknown = JSON.parse(row.value);
      value[key] = parsed;
    } catch {
      return { value, error: "invalidJson" };
    }
  }
  return { value };
}
