import type { TalosTimelinePhase } from "@talos/sdk-core";

import type {
  ChatTimelineItem,
  ChatTurnMetrics,
  ChatUserInput,
  PendingInput,
  SessionUiStatus,
  StreamSessionState,
} from "@/shared/types/ui";

export interface ChatSlice {
  sessionAliases: Record<string, string>;
  transientStatuses: Record<string, SessionUiStatus>;
  streams: Record<string, StreamSessionState>;
  renameSessionLocal: (sessionId: string, title: string) => void;
  deleteSessionAlias: (sessionId: string) => void;
  setSessionStatus: (sessionId: string, status: SessionUiStatus | undefined) => void;
  resetStream: (sessionId: string) => void;
  resetLiveResponse: (sessionId: string) => void;
  upsertUserInputBlock: (
    sessionId: string,
    messageId: string,
    input: ChatUserInput,
  ) => void;
  setPendingInput: (sessionId: string, input?: PendingInput) => void;
  upsertStreamTool: (
    sessionId: string,
    callId: string,
    payload: Partial<StreamSessionState["blocks"][number]> & {
      toolName?: string;
      arguments?: unknown;
      status?: SessionUiStatus | "queued";
      prompt?: string;
    },
  ) => void;
  appendToolProgress: (
    sessionId: string,
    callId: string,
    message: string,
    kind: "status" | "stdout" | "stderr",
  ) => void;
  completeStreamTool: (
    sessionId: string,
    callId: string,
    output: string,
    status: "done" | "error",
    timingMs?: number | null,
  ) => void;
  upsertTimelineItem: (sessionId: string, messageId: string, item: ChatTimelineItem) => void;
  appendTimelineItemText: (
    sessionId: string,
    messageId: string,
    item: ChatTimelineItem,
    chunk: string,
    sequence?: number,
  ) => void;
  commitTimelineAssistantText: (
    sessionId: string,
    messageId: string,
    itemId: string,
    phase: TalosTimelinePhase,
  ) => void;
  updateTimelineTool: (
    sessionId: string,
    messageId: string,
    itemId: string,
    payload: Partial<Extract<ChatTimelineItem, { kind: "tool_call" }>>,
  ) => void;
  completeTimelineItem: (
    sessionId: string,
    messageId: string,
    itemId: string,
    status: ChatTimelineItem["status"],
    completedAtMs: number,
  ) => void;
  setTimelineLive: (sessionId: string, live: boolean) => void;
  setTurnMetrics: (sessionId: string, messageId: string, metrics: ChatTurnMetrics) => void;
  setPendingUserAction: (
    sessionId: string,
    pendingUserAction?: StreamSessionState["pendingUserAction"],
  ) => void;
  setStreamError: (sessionId: string, error?: string) => void;
  setStreamMeta: (
    sessionId: string,
    meta: Partial<
      Pick<
        StreamSessionState,
        | "isStreaming"
        | "activeTurnId"
        | "activeMessageId"
        | "lastEventId"
        | "lastEventSeq"
        | "lastSequence"
        | "connectionState"
        | "reconnectAttempt"
        | "showReconnectControls"
        | "activeInput"
        | "activeClientMessageId"
      >
    >,
  ) => void;
}

export type ChatSliceSetter = (updater: (state: ChatSlice) => Partial<ChatSlice>) => void;
