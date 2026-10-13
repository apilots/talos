import type { ProjectStatus } from "@talos/sdk-core";
export type { ProjectStatus, ProjectStatusState, ProjectUnavailableReason } from "@talos/sdk-core";
import type {
  ContextCompactionMetrics,
  ContextCompactionOutcome,
  ContextCompactionStage,
  ContextCompactionTrigger,
  AgentRunMetrics,
  PendingUserAction,
  TalosContentItem,
  TalosToolProgressKind,
} from "./protocol";

export type DeploymentMode = "cloud" | "local";
export type ReasoningLevel = "none" | "light" | "medium" | "high" | "extra_high";
export type SessionUiStatus = "running" | "done" | "error";

export interface ProjectView {
  nodeId?: string;
  id: string;
  name: string;
  directory?: string | null;
  status?: ProjectStatus | null;
  createdAt: number;
  updatedAt: number;
  origin?: "default" | "server";
  sessionCount?: number;
}

export type ChatToolStatus =
  | "queued"
  | "running"
  | "waiting_dynamic_input"
  | "done"
  | "error"
  | "cancelled";

export type StreamConnectionState =
  | "idle"
  | "awaiting"
  | "streaming"
  | "detached"
  | "reconnecting"
  | "cancelling";

export interface ChatUserInput {
  text: string;
  fileIds: string[];
}

export type ChatRenderBlock =
  | {
      id: string;
      kind: "notice";
      notice: import("@talos/sdk-core").AppProtocol.AgentSystemNotice;
      live?: false;
    }
  | {
      id: string;
      kind: "user";
      text: string;
      fileIds: string[];
      live?: boolean;
    }
  | {
      id: string;
      kind: "assistant" | "thinking" | "system";
      text: string;
      live?: boolean;
    }
  | {
      id: string;
      kind: "status";
      variant: "thinking" | "reconnecting" | "cancelling";
      title: string;
      detail: string;
      live?: boolean;
    }
  | {
      id: string;
      kind: "tool";
      callId: string;
      toolName: string;
      arguments: unknown;
      status: ChatToolStatus;
      output?: string;
      contentItems?: TalosContentItem[];
      progress: Array<{ kind: TalosToolProgressKind; message: string }>;
      prompt?: string;
      timingMs?: number | null;
      live?: boolean;
    }
  | {
      id: string;
      kind: "timeline";
      items: ChatTimelineItem[];
      metrics?: ChatTurnMetrics | null;
      error?: import("@talos/sdk-browser").AppProtocol.ApiError;
      live?: boolean;
    };

export type ChatTurnMetrics = AgentRunMetrics & {
  state: "running" | "completed" | "failed";
};

export type ChatTimelinePhase =
  | "input"
  | "thinking"
  | "pending"
  | "commentary"
  | "final_answer"
  | "acting"
  | "system";

export type ChatTimelineStatus =
  | "queued"
  | "running"
  | "waitingForInput"
  | "completed"
  | "failed"
  | "cancelled";

export type ChatTimelineKind =
  | "user_input"
  | "reasoning"
  | "assistant_text"
  | "execution_summary"
  | "system_notice"
  | "context_compaction"
  | "tool_call";

export type ChatTimelineItem =
  | {
      itemId: string;
      sourceItemId?: string;
      kind: Exclude<ChatTimelineKind, "tool_call" | "context_compaction">;
      phase: ChatTimelinePhase;
      status: ChatTimelineStatus;
      title?: string | null;
      text: string;
      replayFinalAnswer?: boolean;
      notice?: import("@talos/sdk-core").AppProtocol.AgentSystemNotice;
      startedAtMs: number;
      completedAtMs?: number | null;
    }
  | {
      itemId: string;
      sourceItemId?: string;
      kind: "context_compaction";
      phase: ChatTimelinePhase;
      status: ChatTimelineStatus;
      title?: string | null;
      text: string;
      replayFinalAnswer?: boolean;
      trigger: ContextCompactionTrigger;
      stage: ContextCompactionStage;
      outcome?: ContextCompactionOutcome | null;
      metrics?: ContextCompactionMetrics | null;
      startedAtMs: number;
      completedAtMs?: number | null;
    }
  | {
      itemId: string;
      sourceItemId?: string;
      kind: "tool_call";
      phase: ChatTimelinePhase;
      status: ChatTimelineStatus;
      title?: string | null;
      callId: string;
      toolName: string;
      arguments: unknown;
      output?: string;
      contentItems?: TalosContentItem[];
      isError?: boolean;
      progress: Array<{ kind: TalosToolProgressKind; message: string }>;
      prompt?: string;
      startedAtMs: number;
      completedAtMs?: number | null;
    };

export type PendingInputPhase = "local" | "accepted" | "completed";

export interface PendingInput extends ChatUserInput {
  clientMessageId: string;
  serverMessageId?: string;
  phase: PendingInputPhase;
}

export interface SessionInputHistoryItem {
  messageId: string;
  itemIndex: number;
  text: string;
  fileIds: string[];
}

export interface SessionInputHistory {
  committed: SessionInputHistoryItem[];
  pendingInput?: PendingInput;
}

export interface StreamSessionState {
  isStreaming: boolean;
  connectionState?: StreamConnectionState;
  lastEventId?: string;
  lastEventSeq?: number;
  lastSequence?: number;
  activeTurnId?: string;
  activeMessageId?: string;
  reconnectAttempt?: number;
  showReconnectControls?: boolean;
  activeInput?: ChatUserInput;
  activeClientMessageId?: string;
  blocks: ChatRenderBlock[];
  pendingInput?: PendingInput;
  pendingUserAction?: PendingUserAction;
  turnMetrics?: ChatTurnMetrics;
  error?: import("@talos/sdk-browser").AppProtocol.ApiError;
}
