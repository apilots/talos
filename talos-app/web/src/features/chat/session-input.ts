import type {
  AgentUserContent,
  AgentUserInput,
  AutomationMode,
  ModelSelection,
  UserPermissionMode,
} from "@talos/sdk-browser";

import type { ProjectView } from "@/shared/types/ui";

export interface SendTextOptions {
  nodeId?: string;
  modelSelection?: ModelSelection;
  userPermissionMode?: UserPermissionMode;
  project?: ProjectView;
  fileIds?: string[];
  skillSelection?: { name: string; path: string };
}

export function buildQueuedInput(text: string, options?: SendTextOptions): AgentUserInput {
  const content: AgentUserContent[] = [
    ...(options?.skillSelection ? [{ type: "skillSelection" as const, ...options.skillSelection }] : []),
    ...(text ? [{ type: "text" as const, text }] : []),
    ...((options?.fileIds?.length ?? 0) > 0
      ? (options?.fileIds ?? []).map((fileId) => ({ type: "file" as const, fileId }))
      : []),
  ];
  return { content };
}

/** Read this Input's explicit typed selection, never its text or Session mode. */
export function activationModeFromContent(content: readonly AgentUserContent[]): AutomationMode | undefined {
  for (const item of content) {
    if (item.type === "skillSelection" && (item.name === "goal" || item.name === "loop")) return item.name;
  }
  return undefined;
}
