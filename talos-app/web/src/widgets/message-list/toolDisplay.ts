const ARGUMENT_PREVIEW_LIMIT = 94;

const TOOL_ARGUMENT_KEYS: Array<{ names: string[]; keys: string[] }> = [
  { names: ["bash", "shell", "exec", "command"], keys: ["cmd", "command"] },
  { names: ["read", "write", "edit"], keys: ["file_path", "path", "file"] },
  { names: ["grep", "search"], keys: ["pattern", "query"] },
  { names: ["glob"], keys: ["pattern", "glob"] },
  { names: ["web", "reference", "url"], keys: ["url", "domain", "query"] },
];

export function buildToolDisplayTitle(toolName: string, argumentsText: string) {
  const argumentsValue = parseArguments(argumentsText);
  if (!argumentsValue) {
    return toolName;
  }

  const normalizedName = toolName.toLowerCase();
  const preferredKeys = TOOL_ARGUMENT_KEYS.find(({ names }) =>
    names.some((name) => normalizedName.includes(name)),
  )?.keys;
  const previewValue = findStringArgument(argumentsValue, preferredKeys);
  return previewValue ? `${toolName} · ${truncatePreview(previewValue)}` : toolName;
}

function parseArguments(argumentsText: string): Record<string, unknown> | undefined {
  try {
    const value: unknown = JSON.parse(argumentsText);
    return isRecord(value) ? value : undefined;
  } catch {
    return undefined;
  }
}

function findStringArgument(
  argumentsValue: Record<string, unknown>,
  preferredKeys: string[] | undefined,
) {
  for (const key of preferredKeys ?? []) {
    const value = normalizedString(argumentsValue[key]);
    if (value) {
      return value;
    }
  }
  for (const value of Object.values(argumentsValue)) {
    const normalized = normalizedString(value);
    if (normalized) {
      return normalized;
    }
  }
  return undefined;
}

function normalizedString(value: unknown) {
  return typeof value === "string" ? value.trim().replace(/\s+/g, " ") : undefined;
}

function truncatePreview(value: string) {
  const codePoints = Array.from(value);
  if (codePoints.length <= ARGUMENT_PREVIEW_LIMIT) {
    return value;
  }
  return `${codePoints.slice(0, ARGUMENT_PREVIEW_LIMIT - 1).join("")}…`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
