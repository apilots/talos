import { vi } from "vitest";
import type { SessionHistoryResponse } from "@talos/sdk-core";
import { resetTalosClientFactory } from "@/shared/api/client";
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

const apiMocks = vi.hoisted(() => ({
  getSessionHistory: vi.fn(),
  getActiveTurn: vi.fn(),
  subscribeTimelineEvents: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../api", () => ({
  getActiveTurn: apiMocks.getActiveTurn,
  subscribeTimelineEvents: apiMocks.subscribeTimelineEvents,
}));

export function getTimelineEventsMocks() {
  return apiMocks;
}

export const historyOptions = {
  stableEventSeq: 0,
  history: { data: [], nextCursor: null, lastStableEventId: null } as SessionHistoryResponse,
  reloadHistory: () => apiMocks.getSessionHistory("sess-1"),
};

export function resetTimelineEventsTest() {
  apiMocks.subscribeTimelineEvents.mockReset().mockResolvedValue(undefined);
  apiMocks.getActiveTurn.mockReset().mockResolvedValue({ activeTurn: null, lastCompletedEventSeq: 0, latestEventId: null });
  apiMocks.getSessionHistory.mockReset().mockResolvedValue({ data: [], nextCursor: null, lastStableEventId: "event-4" });
  useAppStore.getState().resetStream("sess-1");
}

export function cleanupTimelineEventsTest() {
  vi.useRealTimers();
  resetTalosClientFactory();
}
