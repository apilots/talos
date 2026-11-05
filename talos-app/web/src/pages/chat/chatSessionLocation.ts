import type {
  FileInfo,
  ModelRef,
  UserPermissionMode,
} from "@talos/sdk-browser";

import type { ProjectView, ReasoningLevel } from "@/shared/types/ui";

/** Navigation state accepted when opening a chat session draft. */
export interface SessionLocationState {
  initialNodeId?: string;
  initialPrompt?: string;
  initialSkillSelection?: { name: string; path: string };
  initialModel?: ModelRef;
  initialReasoningLevel?: ReasoningLevel;
  initialPermissionMode?: UserPermissionMode;
  initialProjectId?: string;
  initialProject?: ProjectView;
  initialAttachments?: FileInfo[];
}
