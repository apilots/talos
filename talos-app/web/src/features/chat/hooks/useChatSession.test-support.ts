import { vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { useAppStore } from "@/shared/store/app-store";

vi.hoisted(() => {
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    },
  });
});

const mocks = vi.hoisted(() => ({
  activeTurn: undefined as unknown,
  isStreaming: false,
  activeTurnError: null as Error | null,
  historyLoaded: true,
  activeTurnQuery: vi.fn(),
  activeTurnRefetch: vi.fn().mockResolvedValue(undefined),
  createInitialTurnRequest: vi.fn<typeof import("../api").createInitialTurnRequest>(() => ({
    sessionId: "session-1",
    input: { content: [{ type: "file", fileId: "file-1" }] },
    modelSelection: null,
    userPermissionMode: null,
    projectId: null,
    currentDirectory: null,
    clientRequestId: null,
  })),
  send: vi.fn(),
  sessionHistoryRefetch: vi.fn().mockResolvedValue(undefined),
  messageQueueAdd: vi.fn().mockResolvedValue(undefined),
  messageQueueStart: vi.fn(),
  messageQueueHook: vi.fn(),
  getSendPolicy: vi.fn(),
  submitTurnInput: vi.fn(),
  timeline: vi.fn(),
}));

vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return {
    ...actual,
    createInitialTurnRequest: mocks.createInitialTurnRequest,
    submitTurnInput: mocks.submitTurnInput,
  };
});

vi.mock("./useChatQueries", () => ({
  useActiveTurnQuery: (...args: unknown[]) => {
    mocks.activeTurnQuery(...args);
    return {
      data: mocks.activeTurn === undefined ? undefined : { activeTurn: mocks.activeTurn, lastCompletedEventSeq: 0, latestEventId: null },
      isFetchedAfterMount: true,
      isSuccess: !mocks.activeTurnError,
      error: mocks.activeTurnError,
      refetch: mocks.activeTurnRefetch,
    };
  },
  useSessionHistoryQuery: (_sessionId: string, enabled: boolean) => ({
    data: { data: [], nextCursor: null, lastStableEventId: null },
    error: undefined,
    isSuccess: enabled && mocks.historyLoaded,
    isFetchedAfterMount: enabled && mocks.historyLoaded,
    refetch: mocks.sessionHistoryRefetch,
  }),
}));

vi.mock("@/shared/api/session-message-send-policy", () => ({
  getSessionMessageSendPolicy: mocks.getSendPolicy,
}));

vi.mock("./useSessionBlocks", () => ({
  useSessionBlocks: () => [],
}));

vi.mock("./useSessionQueryInvalidation", () => ({
  useSessionQueryInvalidation: () => ({
    invalidateSessionQueries: async () => {
      await mocks.sessionHistoryRefetch();
      await mocks.activeTurnRefetch();
    },
  }),
}));

vi.mock("./useSessionMessageQueue", () => ({
  useSessionMessageQueue: (...args: unknown[]) => {
    mocks.messageQueueHook(...args);
    return {
    queue: undefined,
    error: undefined,
    add: mocks.messageQueueAdd,
    update: vi.fn(),
    delete: vi.fn(),
    reorder: vi.fn(),
    steer: vi.fn(),
    start: mocks.messageQueueStart,
    };
  },
}));

vi.mock("./useSessionTimelineEvents", () => ({
  useSessionTimelineEvents: (...args: unknown[]) => {
    mocks.timeline(...args);
    return { retryTimeline: vi.fn() };
  },
}));

vi.mock("./useSessionStreaming", () => ({
  useSessionStreaming: () => ({
    canRetryResponse: false,
    isCancelling: false,
    isStreaming: mocks.isStreaming,
    retryResponse: vi.fn(),
    sendMutation: { mutateAsync: mocks.send },
    sendText: vi.fn(),
    stopResponse: vi.fn(),
  }),
}));

export function resetChatSessionMocks() {
  useAppStore.setState({ composerDrafts: {} });
  mocks.send.mockReset().mockRejectedValue(new TalosApiError({
    code: AppProtocol.ErrorCode.ActiveTurnRunning, localization: null,
    message: "Active turn is already running",
  }));
  mocks.createInitialTurnRequest.mockClear();
  mocks.activeTurnRefetch.mockClear();
  mocks.activeTurnQuery.mockClear();
  mocks.sessionHistoryRefetch.mockClear();
  mocks.messageQueueStart.mockReset().mockResolvedValue(undefined);
  mocks.messageQueueAdd.mockReset().mockResolvedValue(undefined);
  mocks.getSendPolicy.mockReset().mockResolvedValue("queue");
  mocks.submitTurnInput.mockReset().mockResolvedValue(undefined);
  mocks.messageQueueHook.mockClear();
  mocks.timeline.mockClear();
  mocks.isStreaming = false;
  mocks.activeTurn = undefined;
  mocks.activeTurnError = null;
  mocks.historyLoaded = true;
  useAppStore.getState().resetStream("session-1");
}

export function getChatSessionMocks() {
  return mocks;
}
