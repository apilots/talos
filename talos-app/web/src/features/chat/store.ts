import type {
  ApprovalMode,
  PendingApproval,
  PendingInput,
  SessionUiStatus,
  StreamSessionState,
} from "@/shared/types/ui";

type StreamTextKind = "assistant" | "thinking" | "system";

export interface ChatSlice {
  sessionAliases: Record<string, string>;
  sessionApprovalModes: Record<string, ApprovalMode>;
  transientStatuses: Record<string, SessionUiStatus>;
  streams: Record<string, StreamSessionState>;
  renameSessionLocal: (sessionId: string, title: string) => void;
  deleteSessionAlias: (sessionId: string) => void;
  setSessionApprovalMode: (sessionId: string, mode: ApprovalMode) => void;
  setSessionStatus: (sessionId: string, status: SessionUiStatus | undefined) => void;
  resetStream: (sessionId: string) => void;
  resetLiveResponse: (sessionId: string) => void;
  appendStreamText: (
    sessionId: string,
    kind: StreamTextKind,
    chunk: string,
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
  setPendingUserAction: (
    sessionId: string,
    pendingUserAction?: StreamSessionState["pendingUserAction"],
  ) => void;
  setPendingApproval: (sessionId: string, approval?: PendingApproval) => void;
  setStreamError: (sessionId: string, error?: string) => void;
  setStreamMeta: (
    sessionId: string,
    meta: Partial<
      Pick<
        StreamSessionState,
        | "isStreaming"
        | "activeMessageId"
        | "lastSequence"
        | "connectionState"
        | "expectedMessageCount"
        | "reconnectAttempt"
        | "showReconnectControls"
        | "activeInputText"
        | "activeClientMessageId"
        | "awaitingFinalResponse"
      >
    >,
  ) => void;
}

export type ChatSliceSetter = (updater: (state: ChatSlice) => Partial<ChatSlice>) => void;

function ensureStreamState(state: Pick<ChatSlice, "streams">, sessionId: string): StreamSessionState {
  return (
    state.streams[sessionId] ?? {
      isStreaming: false,
      connectionState: "idle",
      blocks: [],
    }
  );
}

export function createChatSlice(set: ChatSliceSetter): ChatSlice {
  return {
    sessionAliases: {},
    sessionApprovalModes: {},
    transientStatuses: {},
    streams: {},
    renameSessionLocal: (sessionId, title) =>
      set((state) => ({
        sessionAliases: {
          ...state.sessionAliases,
          [sessionId]: title,
        },
      })),
    deleteSessionAlias: (sessionId) =>
      set((state) => {
        const aliases = { ...state.sessionAliases };
        delete aliases[sessionId];
        return { sessionAliases: aliases };
      }),
    setSessionApprovalMode: (sessionId, mode) =>
      set((state) => ({
        sessionApprovalModes: {
          ...state.sessionApprovalModes,
          [sessionId]: mode,
        },
      })),
    setSessionStatus: (sessionId, status) =>
      set((state) => {
        const next = { ...state.transientStatuses };
        if (!status) {
          delete next[sessionId];
        } else {
          next[sessionId] = status;
        }
        return { transientStatuses: next };
      }),
    resetStream: (sessionId) =>
      set((state) => ({
        streams: {
          ...state.streams,
          [sessionId]: { isStreaming: false, connectionState: "idle", blocks: [] },
        },
      })),
    resetLiveResponse: (sessionId) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        return {
          streams: {
            ...state.streams,
            [sessionId]: {
              ...stream,
              isStreaming: false,
              connectionState: "idle",
              blocks: [],
              pendingApproval: undefined,
              pendingUserAction: undefined,
              awaitingFinalResponse: false,
              error: undefined,
            },
          },
        };
      }),
    appendStreamText: (sessionId, kind, chunk) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = [...stream.blocks];
        const lastBlock = blocks.at(-1);
        if (
          lastBlock &&
          (lastBlock.kind === "assistant" ||
            lastBlock.kind === "thinking" ||
            lastBlock.kind === "system") &&
          lastBlock.kind === kind
        ) {
          blocks[blocks.length - 1] = {
            ...lastBlock,
            text: lastBlock.text + chunk,
            live: true,
          };
        } else {
          blocks.push({
            id: `${kind}-${Date.now()}-${blocks.length}`,
            kind,
            text: chunk,
            live: true,
          });
        }
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, isStreaming: true, blocks },
          },
        };
      }),
    setPendingInput: (sessionId, pendingInput) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, pendingInput },
          },
        };
      }),
    upsertStreamTool: (sessionId, callId, payload) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = [...stream.blocks];
        const index = blocks.findIndex(
          (block) => block.kind === "tool" && block.callId === callId,
        );
        if (index >= 0) {
          const current = blocks[index];
          if (current.kind === "tool") {
            blocks[index] = {
              ...current,
              toolName: payload.toolName ?? current.toolName,
              arguments: payload.arguments ?? current.arguments,
              status: (payload.status as typeof current.status | undefined) ?? current.status,
              prompt: payload.prompt ?? current.prompt,
              live: true,
            };
          }
        } else {
          blocks.push({
            id: `tool-${callId}`,
            kind: "tool",
            callId,
            toolName: payload.toolName ?? "tool",
            arguments: payload.arguments ?? null,
            status:
              (payload.status as
                | "queued"
                | "running"
                | "waiting_approval"
                | "done"
                | "error"
                | undefined) ?? "queued",
            progress: [],
            prompt: payload.prompt,
            live: true,
          });
        }
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, isStreaming: true, blocks },
          },
        };
      }),
    appendToolProgress: (sessionId, callId, message, kind) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = stream.blocks.map((block) => {
          if (block.kind !== "tool" || block.callId !== callId) {
            return block;
          }
          return {
            ...block,
            progress: [...block.progress, { kind, message }],
            live: true,
          };
        });
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, blocks },
          },
        };
      }),
    completeStreamTool: (sessionId, callId, output, status, timingMs) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = stream.blocks.map((block) => {
          if (block.kind !== "tool" || block.callId !== callId) {
            return block;
          }
          return {
            ...block,
            output,
            status,
            timingMs,
            live: true,
          };
        });
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, blocks },
          },
        };
      }),
    setPendingUserAction: (sessionId, pendingUserAction) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, pendingUserAction },
          },
        };
      }),
    setPendingApproval: (sessionId, pendingApproval) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, pendingApproval },
          },
        };
      }),
    setStreamError: (sessionId, error) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        return {
          streams: {
            ...state.streams,
            [sessionId]: error
              ? { ...stream, error, isStreaming: false, connectionState: "idle" }
              : { ...stream, error: undefined },
          },
        };
      }),
    setStreamMeta: (sessionId, meta) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, ...meta },
          },
        };
      }),
  };
}
