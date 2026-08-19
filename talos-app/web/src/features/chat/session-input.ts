import type {
  ModelSelection,
  SkillSuggestion,
  TalosInputItem,
  UserPermissionMode,
} from "@talos/sdk-browser";

import type { ProjectView } from "@/shared/types/ui";

export interface SendTextOptions {
  modelSelection?: ModelSelection;
  userPermissionMode?: UserPermissionMode;
  project?: ProjectView;
  skills?: SkillSuggestion[];
  fileIds?: string[];
}

export function buildQueuedInput(text: string, options?: SendTextOptions): TalosInputItem[] {
  return [
    ...(text ? [{ type: "text" as const, text }] : []),
    ...((options?.fileIds?.length ?? 0) > 0
      ? [{ type: "files" as const, fileIds: options?.fileIds ?? [] }]
      : []),
    ...(options?.skills ?? []).map((skill) => ({
      type: "skill_selection" as const,
      name: skill.name,
      path: skill.path,
    })),
  ];
}
