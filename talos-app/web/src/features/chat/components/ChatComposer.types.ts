import type {
  AgentUserInput,
  FileInfo,
  MessageQueue,
  UserPermissionMode,
} from "@talos/sdk-browser";

import type {
  ProjectView,
  ReasoningLevel,
  SessionInputHistory,
} from "@/shared/types/ui";
import type { ComposerSessionCommandId } from "./composer-input/composer-suggestions";
import type { ComposerAttachmentKind } from "./useComposerAttachments";
import type {
  ComposerCommandResponseState,
  ComposerStatusSnapshot,
} from "./command-response";

export type { ReasoningLevel };

export interface MessageQueueController {
  activeTurnId?: string;
  error?: string;
  queue?: MessageQueue;
  delete: (messageId: string) => Promise<unknown>;
  reorder: (messageIds: string[]) => Promise<unknown>;
  start: (messageId?: string) => Promise<unknown>;
  steer: (messageId: string, turnId: string) => Promise<unknown>;
  update: (messageId: string, input: AgentUserInput) => Promise<unknown>;
}

export interface ChatComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: (attachments: FileInfo[]) => Promise<void> | void;
  onCommand?: (command: ComposerSessionCommandId) => Promise<void> | void;
  commandResponse?: ComposerCommandResponseState;
  commandStatus?: ComposerStatusSnapshot;
  onDismissCommandResponse?: () => void;
  onCancel?: () => void;
  inputHistory?: SessionInputHistory;
  disabled?: boolean;
  permissionMode?: UserPermissionMode;
  permissionModeDisabled?: boolean;
  onPermissionModeChange?: (mode: UserPermissionMode) => void;
  isStreaming?: boolean;
  isCancelling?: boolean;
  contextUsage?: import("@talos/sdk-browser").SessionInfo["usage"]["context"];
  sessionTotalTokens?: number;
  models: Array<{ id: string; name: string; provider: string }>;
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  projects: ProjectView[];
  activeProjectId: string;
  onProjectChange: (projectId: string) => void;
  onCreateProject?: (input: string | { directory: string; name?: string }) => Promise<void>;
  reasoningLevel?: ReasoningLevel;
  onReasoningLevelChange?: (level: ReasoningLevel) => void;
  reasoningEnabled?: boolean;
  showProjectSelector?: boolean;
  compact?: boolean;
  initialAttachments?: FileInfo[];
  supportedAttachmentKinds?: ComposerAttachmentKind[];
  messageQueue?: MessageQueueController;
}
