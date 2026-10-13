import type {
  AgentUserContent,
  AgentUserInput,
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
}

export function buildQueuedInput(text: string, options?: SendTextOptions): AgentUserInput {
  const content: AgentUserContent[] = [
    ...(text ? [{ type: "text" as const, text }] : []),
    ...((options?.fileIds?.length ?? 0) > 0
      ? (options?.fileIds ?? []).map((fileId) => ({ type: "file" as const, fileId }))
      : []),
  ];
  return { content };
}
