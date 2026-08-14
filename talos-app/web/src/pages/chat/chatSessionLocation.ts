import type {
  FileInfo,
  ModelRef,
  SkillSuggestion,
  UserPermissionMode,
} from "@talos/sdk-browser";

import type { ProjectView, ReasoningLevel } from "@/shared/types/ui";

/** Navigation state accepted when opening a chat session draft. */
export interface SessionLocationState {
  initialPrompt?: string;
  initialModel?: ModelRef;
  initialReasoningLevel?: ReasoningLevel;
  initialPermissionMode?: UserPermissionMode;
  initialProjectId?: string;
  initialProject?: ProjectView;
  initialSkills?: SkillSuggestion[];
  initialAttachments?: FileInfo[];
}
