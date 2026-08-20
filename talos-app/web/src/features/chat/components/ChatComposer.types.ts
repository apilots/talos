import type {
  FileInfo,
  MessageQueue,
  SkillSuggestion,
  TalosInputItem,
  UserPermissionMode,
} from "@talos/sdk-browser";

import type { ContextCompactionStage } from "@/shared/types/protocol";
import type {
  ProjectView,
  ReasoningLevel,
  RuntimeMode,
  SessionInputHistory,
} from "@/shared/types/ui";
import type { ComposerCommand } from "./composer-commands";
import type { ComposerAttachmentKind } from "./useComposerAttachments";

export type { ReasoningLevel, RuntimeMode };

export type ProjectMenuMode = "list" | "add_menu";

export interface MessageQueueController {
  activeTurnId?: string;
  error?: string;
  queue?: MessageQueue;
  delete: (messageId: string) => Promise<unknown>;
  reorder: (messageIds: string[]) => Promise<unknown>;
  start: (messageId?: string) => Promise<unknown>;
  steer: (messageId: string, turnId: string) => Promise<unknown>;
  update: (messageId: string, input: TalosInputItem[]) => Promise<unknown>;
}

export interface ChatComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: (skills: SkillSuggestion[], attachments: FileInfo[]) => Promise<void> | void;
  onCommand?: (command: ComposerCommand) => Promise<void> | void;
  commandNotice?: string;
  initialSelectedSkills?: SkillSuggestion[];
  onCancel?: () => void;
  inputHistory?: SessionInputHistory;
  disabled?: boolean;
  permissionMode?: UserPermissionMode;
  permissionModeDisabled?: boolean;
  onPermissionModeChange?: (mode: UserPermissionMode) => void;
  isStreaming?: boolean;
  isCancelling?: boolean;
  contextCompactionStage?: ContextCompactionStage;
  contextUsage?: import("@talos/sdk-browser").SessionInfo["usage"]["context"];
  sessionTotalTokens?: number;
  models: Array<{ id: string; name: string; provider: string }>;
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  projects: ProjectView[];
  activeProjectId: string;
  onProjectChange: (projectId: string) => void;
  onCreateProject?: (input: string | { directory: string; name?: string }) => Promise<void>;
  runtimeMode: RuntimeMode;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
  reasoningLevel?: ReasoningLevel;
  onReasoningLevelChange?: (level: ReasoningLevel) => void;
  reasoningEnabled?: boolean;
  showProjectSelector?: boolean;
  showRuntimeModeSelector?: boolean;
  compact?: boolean;
  initialAttachments?: FileInfo[];
  supportedAttachmentKinds?: ComposerAttachmentKind[];
  messageQueue?: MessageQueueController;
}
