import type { ReasoningEffort, StartTurnRequest, UserPermissionMode } from "@talos/sdk-core";

const permissionModes = {
  askForApproval: true, approveForMe: true, fullAccess: true,
} satisfies Record<UserPermissionMode, true>;
const reasoningEfforts = {
  light: true, medium: true, high: true, xhigh: true, max: true,
} satisfies Record<ReasoningEffort, true>;

function isNullableEnum(value: unknown, values: Record<string, true>): boolean {
  return value === null || (typeof value === "string" && Object.hasOwn(values, value));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

/** Restore only the input variants produced by the new-chat composer. */
export function restoreComposerRequest(body: string): StartTurnRequest {
  const value: unknown = JSON.parse(body);
  if (!isRecord(value) || typeof value.sessionId !== "string"
    || !(value.nodeId === undefined || typeof value.nodeId === "string")
    || !isNullableString(value.projectId) || !isNullableString(value.currentDirectory)
    || !isNullableString(value.clientRequestId)
    || !isNullableEnum(value.userPermissionMode, permissionModes)
    || !isRecord(value.input) || !Array.isArray(value.input.content)) {
    throw new Error("Invalid saved composer request.");
  }
  const content: unknown[] = value.input.content;
  for (const item of content) {
    if (!isRecord(item) || !(
      (item.type === "text" && typeof item.text === "string")
      || (item.type === "file" && typeof item.fileId === "string")
      || (item.type === "skillSelection" && typeof item.name === "string" && typeof item.path === "string")
    )) throw new Error("Invalid saved composer input.");
  }
  const selection = value.modelSelection;
  if (selection !== null && (!isRecord(selection) || !isRecord(selection.modelRef)
    || typeof selection.modelRef.provider !== "string" || typeof selection.modelRef.id !== "string"
    || !isNullableEnum(selection.reasoningEffort, reasoningEfforts))) {
    throw new Error("Invalid saved composer model selection.");
  }
  return value as StartTurnRequest;
}
