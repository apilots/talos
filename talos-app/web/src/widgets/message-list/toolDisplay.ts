const ARGUMENT_PREVIEW_LIMIT = 94;
const COMMAND_ARGUMENT_KEYS = ["command", "cmd"];

const TOOL_ARGUMENT_KEYS: Array<{ names: string[]; keys: string[] }> = [
  { names: ["bash", "shell", "exec", "command"], keys: COMMAND_ARGUMENT_KEYS },
  { names: ["read", "write", "edit"], keys: ["path", "file_path", "file"] },
  { names: ["grep", "search"], keys: ["pattern", "query"] },
  { names: ["glob"], keys: ["pattern", "glob"] },
  { names: ["web", "reference", "url"], keys: ["url", "domain", "query"] },
];

export function buildToolDisplayTitle(toolName: string, input: unknown) {
  const previewValue = toolDisplayTarget(toolName, input);
  return previewValue ? `${toolName} · ${previewValue}` : toolName;
}

export function toolDisplayTarget(toolName: string, input: unknown, basename = false) {
  const argumentsValue = resolveArguments(input);
  if (!argumentsValue) {
    return undefined;
  }

  const normalizedName = toolName.toLowerCase();
  const preferredKeys = TOOL_ARGUMENT_KEYS.find(({ names }) =>
    names.some((name) => normalizedName.includes(name)),
  )?.keys;
  const previewValue = findStringArgument(argumentsValue, preferredKeys);
  if (!previewValue) return undefined;
  const target = basename ? previewValue.split(/[/\\]/).at(-1) || previewValue : previewValue;
  return truncatePreview(target);
}

export function toolCommand(input: unknown) {
  const args = resolveArguments(input);
  return COMMAND_ARGUMENT_KEYS.map((key) => args?.[key])
    .find((value): value is string => typeof value === "string" && !!value.trim());
}

export function toolSearchQuery(input: unknown) {
  const query = normalizedString(resolveArguments(input)?.query);
  return query ? truncatePreview(query) : undefined;
}

export function toolActivity(toolName: string) {
  switch (toolName) {
    case "Bash": return "commands";
    case "Read": return "read";
    case "Edit":
    case "Write": return "edit";
    case "Grep":
    case "Glob": return "search";
    case "WebFetch":
    case "WebSearch": return "web";
    case "ToolSearch": return "lookup";
    default: return undefined;
  }
}

function resolveArguments(input: unknown): Record<string, unknown> | undefined {
  if (typeof input !== "string") return isRecord(input) ? input : undefined;
  try {
    const value: unknown = JSON.parse(input);
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
