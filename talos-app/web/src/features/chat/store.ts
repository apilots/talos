import type { StreamSessionState } from "@/shared/types/ui";
import {
  appendTimelineItemTextInStream,
  commitTimelineAssistantTextInStream,
  completeTimelineItemInStream,
  replaceTimelineItemInStream,
  setTimelineLiveInStream,
  setTurnMetricsInStream,
  updateTimelineToolInStream,
  upsertTimelineItemInStream,
} from "./timeline-state";
import type { ChatSlice, ChatSliceSetter } from "./store.types";

export type { ChatSlice, ChatSliceSetter } from "./store.types";

function ensureStreamState(state: Pick<ChatSlice, "streams">, sessionId: string): StreamSessionState {
  return (
    state.streams[sessionId] ?? {
      isStreaming: false,
      connectionState: "idle",
      blocks: [],
    }
  );
}

function updateStream(
  set: ChatSliceSetter,
  sessionId: string,
  update: (stream: StreamSessionState) => StreamSessionState,
): void {
  set((state) => ({
    streams: { ...state.streams, [sessionId]: update(ensureStreamState(state, sessionId)) },
  }));
}

export function createChatSlice(set: ChatSliceSetter): ChatSlice {
  return {
    sessionAliases: {},
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
              turnMetrics: undefined,
              activeMessageId: undefined,
              pendingUserAction: undefined,
              error: undefined,
            },
          },
        };
      }),
    upsertUserInputBlock: (sessionId, turnId, messageId, input) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = [...stream.blocks];
        const id = `user-${turnId}-${messageId}`;
        const index = blocks.findIndex((block) => block.id === id && block.kind === "user");
        if (index >= 0) {
          const current = blocks[index];
          if (current.kind === "user") {
            blocks[index] = { ...current, ...input, live: false };
          }
        } else {
          blocks.push({
            id,
            kind: "user",
            ...input,
            live: false,
          });
        }
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, blocks },
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
                | "waiting_dynamic_input"
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
    upsertTimelineItem: (sessionId, turnId, item) =>
      updateStream(set, sessionId, (stream) =>
        upsertTimelineItemInStream(stream, sessionId, turnId, item),
      ),
    replaceTimelineItem: (sessionId, turnId, item) =>
      updateStream(set, sessionId, (stream) =>
        replaceTimelineItemInStream(stream, sessionId, turnId, item),
      ),
    appendTimelineItemText: (sessionId, turnId, sourceItem, chunk, sequence) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        return {
          streams: {
            ...state.streams,
            [sessionId]: appendTimelineItemTextInStream(
              stream,
              sessionId,
              turnId,
              sourceItem,
              chunk,
              sequence,
            ),
          },
        };
      }),
    commitTimelineAssistantText: (sessionId, turnId, itemId, phase) =>
      updateStream(set, sessionId, (stream) =>
        commitTimelineAssistantTextInStream(stream, sessionId, turnId, itemId, phase),
      ),
    updateTimelineTool: (sessionId, turnId, itemId, payload) =>
      updateStream(set, sessionId, (stream) =>
        updateTimelineToolInStream(stream, sessionId, turnId, itemId, payload),
      ),
    completeTimelineItem: (sessionId, turnId, itemId, status, completedAtMs) =>
      updateStream(set, sessionId, (stream) =>
        completeTimelineItemInStream(stream, sessionId, turnId, itemId, status, completedAtMs),
      ),
    setTimelineLive: (sessionId, live) =>
      updateStream(set, sessionId, (stream) => setTimelineLiveInStream(stream, live)),
    setTurnMetrics: (sessionId, turnId, metrics) =>
      updateStream(set, sessionId, (stream) =>
        setTurnMetricsInStream(stream, sessionId, turnId, metrics),
      ),
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
