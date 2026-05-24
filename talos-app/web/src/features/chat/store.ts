import type {
  ChatRenderBlock,
  ChatTimelineItem,
  PendingInput,
  SessionUiStatus,
  StreamSessionState,
} from "@/shared/types/ui";
import type { TalosTimelinePhase } from "@talos/sdk-core";

type StreamTextKind = "assistant" | "thinking" | "system";

export interface ChatSlice {
  sessionAliases: Record<string, string>;
  transientStatuses: Record<string, SessionUiStatus>;
  streams: Record<string, StreamSessionState>;
  renameSessionLocal: (sessionId: string, title: string) => void;
  deleteSessionAlias: (sessionId: string) => void;
  setSessionStatus: (sessionId: string, status: SessionUiStatus | undefined) => void;
  resetStream: (sessionId: string) => void;
  resetLiveResponse: (sessionId: string) => void;
  appendStreamText: (
    sessionId: string,
    kind: StreamTextKind,
    chunk: string,
  ) => void;
  upsertUserInputBlock: (sessionId: string, messageId: string, text: string) => void;
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
  upsertTimelineItem: (sessionId: string, item: ChatTimelineItem) => void;
  appendTimelineItemText: (sessionId: string, item: ChatTimelineItem, chunk: string, sequence?: number) => void;
  commitTimelineAssistantText: (
    sessionId: string,
    itemId: string,
    phase: TalosTimelinePhase,
  ) => void;
  updateTimelineTool: (
    sessionId: string,
    itemId: string,
    payload: Partial<Extract<ChatTimelineItem, { kind: "tool_call" }>>,
  ) => void;
  completeTimelineItem: (
    sessionId: string,
    itemId: string,
    status: ChatTimelineItem["status"],
    completedAtMs: number,
  ) => void;
  setTimelineLive: (sessionId: string, live: boolean) => void;
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
        | "activeMessageId"
        | "lastEventId"
        | "lastEventSeq"
        | "lastSequence"
        | "connectionState"
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

function ensureTimelineBlock(blocks: StreamSessionState["blocks"], sessionId: string) {
  const existingIndex = blocks.findIndex((block) => block.kind === "timeline");
  if (existingIndex >= 0) {
    return existingIndex;
  }
  blocks.push({
    id: `timeline-${sessionId}`,
    kind: "timeline",
    items: [],
    live: true,
  });
  return blocks.length - 1;
}

function mergeTimelineItem(
  current: ChatTimelineItem,
  next: ChatTimelineItem,
): ChatTimelineItem {
  if (current.kind === "tool_call" && next.kind === "tool_call") {
    const status =
      current.status === "waiting_dynamic_input" && next.status === "running"
        ? current.status
        : next.status;
    return {
      ...current,
      ...next,
      status,
      progress: next.progress.length > 0 ? next.progress : current.progress,
      output: next.output ?? current.output,
      prompt: next.prompt ?? current.prompt,
      completedAtMs: next.completedAtMs ?? current.completedAtMs,
    };
  }
  if (current.kind !== "tool_call" && next.kind !== "tool_call") {
    return {
      ...current,
      ...next,
      text: next.text || current.text,
      completedAtMs: next.completedAtMs ?? current.completedAtMs,
    };
  }
  return next;
}

type TextTimelineItem = Extract<ChatTimelineItem, { text: string }>;

function isTextTimelineItem(item: ChatTimelineItem): item is TextTimelineItem {
  return item.kind !== "tool_call";
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
    upsertUserInputBlock: (sessionId, messageId, text) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = [...stream.blocks];
        const id = `user-${messageId}`;
        const index = blocks.findIndex((block) => block.id === id && block.kind === "user");
        if (index >= 0) {
          const current = blocks[index];
          if (current.kind === "user") {
            blocks[index] = { ...current, text, live: false };
          }
        } else {
          blocks.push({
            id,
            kind: "user",
            text,
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
    upsertTimelineItem: (sessionId, item) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = [...stream.blocks];
        const timelineIndex = ensureTimelineBlock(blocks, sessionId);
        const timeline = blocks[timelineIndex];
        if (timeline.kind !== "timeline") {
          return {};
        }
        const existingIndex = timeline.items.findIndex((entry) => entry.itemId === item.itemId);
        const items = [...timeline.items];
        if (existingIndex >= 0) {
          items[existingIndex] = mergeTimelineItem(items[existingIndex], item);
        } else {
          items.push(item);
        }
        blocks[timelineIndex] = { ...timeline, items, live: true };
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, isStreaming: true, blocks },
          },
        };
      }),
    appendTimelineItemText: (sessionId, sourceItem, chunk, sequence) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = [...stream.blocks];
        const timelineIndex = ensureTimelineBlock(blocks, sessionId);
        const timeline = blocks[timelineIndex];
        if (timeline.kind !== "timeline") {
          return {};
        }
        const sourceItemId = sourceItem.sourceItemId ?? sourceItem.itemId;
        const sameSource = (item: ChatTimelineItem): item is TextTimelineItem =>
          isTextTimelineItem(item) && (item.sourceItemId ?? item.itemId) === sourceItemId;
        const items = timeline.items.filter((item) => !sameSource(item) || item.text.trim() !== "");
        const lastItem = items.at(-1);
        if (lastItem && sameSource(lastItem)) {
          items[items.length - 1] = { ...lastItem, text: lastItem.text + chunk };
        } else if (sourceItem.kind !== "tool_call") {
          const hasExistingSegment = items.some(sameSource);
          items.push({
            ...sourceItem,
            itemId: hasExistingSegment ? `${sourceItemId}::segment-${sequence ?? items.length}` : sourceItemId,
            sourceItemId,
            text: chunk,
          });
        }
        blocks[timelineIndex] = { ...timeline, items, live: true };
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, isStreaming: true, blocks },
          },
        };
      }),
    commitTimelineAssistantText: (sessionId, itemId, phase) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        let movedText = "";
        const blocks: ChatRenderBlock[] = [];
        for (const block of stream.blocks) {
          if (block.kind !== "timeline") {
            blocks.push(block);
            continue;
          }
          const items: ChatTimelineItem[] = [];
          for (const item of block.items) {
            const sameSource = item.itemId === itemId || item.sourceItemId === itemId;
            if (!sameSource) {
              items.push(item);
              continue;
            }
            if (item.kind !== "tool_call") {
              movedText += item.text;
            }
            if (phase !== "final_answer") {
              items.push({ ...item, phase });
            }
          }
          if (items.length === 0 && phase === "final_answer") {
            continue;
          }
          blocks.push({ ...block, items, live: phase !== "final_answer" });
        }
        const nextBlocks = [...blocks];
        if (phase === "final_answer" && movedText.trim()) {
          const lastBlock = nextBlocks.at(-1);
          if (lastBlock?.kind === "assistant") {
            nextBlocks[nextBlocks.length - 1] = {
              ...lastBlock,
              text: lastBlock.text + movedText,
              live: true,
            };
          } else {
            nextBlocks.push({
              id: `assistant-${Date.now()}-${nextBlocks.length}`,
              kind: "assistant",
              text: movedText,
              live: true,
            });
          }
        }
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, isStreaming: true, blocks: nextBlocks },
          },
        };
      }),
    updateTimelineTool: (sessionId, itemId, payload) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = stream.blocks.map((block) => {
          if (block.kind !== "timeline") {
            return block;
          }
          return {
            ...block,
            live: true,
            items: block.items.map((item) => {
              if (item.itemId !== itemId || item.kind !== "tool_call") {
                return item;
              }
              return {
                ...item,
                ...payload,
                progress: payload.progress ?? item.progress,
              };
            }),
          };
        });
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, isStreaming: true, blocks },
          },
        };
      }),
    completeTimelineItem: (sessionId, itemId, status, completedAtMs) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = stream.blocks.map((block) => {
          if (block.kind !== "timeline") {
            return block;
          }
          return {
            ...block,
            items: block.items.map((item) =>
              (item.itemId === itemId || item.sourceItemId === itemId)
                ? { ...item, status, completedAtMs }
                : item,
            ),
          };
        });
        return {
          streams: {
            ...state.streams,
            [sessionId]: { ...stream, blocks },
          },
        };
      }),
    setTimelineLive: (sessionId, live) =>
      set((state) => {
        const stream = ensureStreamState(state, sessionId);
        const blocks = stream.blocks.map((block) =>
          block.kind === "timeline" ? { ...block, live } : block,
        );
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
