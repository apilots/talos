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
import type { ComposerModelOption } from "../model-selection";
import type {
  ComposerCommandResponseState,
  ComposerStatusSnapshot,
} from "./command-response";

export type { ReasoningLevel };

export interface MessageQueueController {
  activeTurnId?: string;
  canSteer?: boolean;
  canStart?: boolean;
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
  initialAutomationMode?: import("@talos/sdk-browser").AutomationMode;
  onRestoredAutomationModeConsumed?: () => void;
  automationCommands?: import("@talos/sdk-browser").AppProtocol.AutomationCommandDescriptor[];
  onDraftChange: (value: string) => void;
  automation?: import("@/features/automation/useSessionAutomation").SessionAutomationController;
  onSend: (attachments: FileInfo[], mode?: import("@talos/sdk-browser").AutomationMode) => Promise<void> | void;
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
  models: ComposerModelOption[];
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  projects: ProjectView[];
  activeProjectId: string;
  onProjectChange: (projectId: string) => void;
  onCreateProject?: (input: string | { directory: string; name?: string }) => Promise<void>;
  reasoningLevel?: ReasoningLevel;
  onReasoningLevelChange?: (level: ReasoningLevel) => void;
  showProjectSelector?: boolean;
  compact?: boolean;
  initialAttachments?: FileInfo[];
  supportedAttachmentKinds?: ComposerAttachmentKind[];
  messageQueue?: MessageQueueController;
}
