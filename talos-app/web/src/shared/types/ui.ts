import type {
  ContextCompactionStage,
  TalosContentItem,
  TalosPendingUserAction,
  TalosTimelineItemKind,
  TalosTimelineItemStatus,
  TalosTimelinePhase,
  TalosToolProgressKind,
  TalosTurnExecutionMetrics,
} from "./protocol";

export type RuntimeMode = "cloud" | "local";
export type ReasoningLevel = "none" | "light" | "medium" | "high" | "extra_high";
export type SessionUiStatus = "running" | "done" | "error";

export interface ProjectView {
  id: string;
  name: string;
  directory?: string | null;
  status?: ProjectStatus | null;
  createdAt: number;
  updatedAt: number;
  origin?: "default" | "server";
  sessionCount?: number;
}

export type ProjectStatusState = "available" | "missing" | "not_directory" | "permission_denied";
export type ProjectUnavailableReason = "path_not_found" | "not_directory" | "permission_denied";

export interface ProjectStatus {
  state: ProjectStatusState;
  reason?: ProjectUnavailableReason | null;
}

export interface MemoryItemView {
  id: string;
  title: string;
  detail: string;
  tags: string[];
  updatedAt: number;
}

export type ChatToolStatus =
  | "queued"
  | "running"
  | "waiting_dynamic_input"
  | "done"
  | "error";

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
      live?: boolean;
    };

export type ChatTurnMetrics = TalosTurnExecutionMetrics & {
  state: "running" | "completed" | "failed";
};

export type ChatTimelineItem =
  | {
      itemId: string;
      sourceItemId?: string;
      kind: Exclude<TalosTimelineItemKind, "tool_call" | "context_compaction">;
      phase: TalosTimelinePhase;
      status: TalosTimelineItemStatus;
      title?: string | null;
      text: string;
      replayFinalAnswer?: boolean;
      startedAtMs: number;
      completedAtMs?: number | null;
    }
  | {
      itemId: string;
      sourceItemId?: string;
      kind: "context_compaction";
      phase: TalosTimelinePhase;
      status: TalosTimelineItemStatus;
      title?: string | null;
      text: string;
      replayFinalAnswer?: boolean;
      stage: ContextCompactionStage;
      startedAtMs: number;
      completedAtMs?: number | null;
    }
  | {
      itemId: string;
      sourceItemId?: string;
      kind: "tool_call";
      phase: TalosTimelinePhase;
      status: TalosTimelineItemStatus;
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
  activeMessageId?: string;
  reconnectAttempt?: number;
  showReconnectControls?: boolean;
  activeInput?: ChatUserInput;
  activeClientMessageId?: string;
  awaitingFinalResponse?: boolean;
  blocks: ChatRenderBlock[];
  pendingInput?: PendingInput;
  pendingUserAction?: TalosPendingUserAction;
  turnMetrics?: ChatTurnMetrics;
  error?: string;
}
