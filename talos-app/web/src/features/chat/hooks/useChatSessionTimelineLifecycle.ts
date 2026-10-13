import { useCallback, useState } from "react";

import {
  type useActiveTurnQuery,
  type useSessionHistoryQuery,
} from "./useChatQueries";
import { useSessionTimelineEvents } from "./useSessionTimelineEvents";
import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import type { useSessionMessageQueue } from "./useSessionMessageQueue";
import {
  activeContextCompaction,
  isContextCompactionRunning,
  latestContextCompaction,
} from "../timeline-state";
import type { StreamSessionState } from "@/shared/types/ui";

interface UseChatSessionTimelineLifecycleOptions {
  activeTurnQuery: ReturnType<typeof useActiveTurnQuery>;
  sessionHistoryQuery: ReturnType<typeof useSessionHistoryQuery>;
  sessionId: string;
  sessionAvailable: boolean;
  streamState?: StreamSessionState;
  messageQueue: ReturnType<typeof useSessionMessageQueue>;
}

/** Owns Session-scoped Timeline recovery, maintenance observation, and compaction selection. */
export function useChatSessionTimelineLifecycle({
  activeTurnQuery,
  sessionHistoryQuery,
  sessionId,
  sessionAvailable,
  streamState,
  messageQueue,
}: UseChatSessionTimelineLifecycleOptions) {
  const [observedMaintenanceSessionId, setObservedMaintenanceSessionId] = useState<string>();
  const { invalidateSessionQueries } = useSessionQueryInvalidation(sessionId);
  const history = sessionHistoryQuery.data;
  const activeTurn = activeTurnQuery.data?.activeTurn;
  const streamedTurnIsComplete = history?.data.some((turn) =>
    turn.identity.turnId === streamState?.activeTurnId,
  );
  const hasRemoteActiveTurn = activeTurnQuery.isFetchedAfterMount &&
    isLiveRecoverableActiveTurn(activeTurn?.state) &&
    !history?.data.some((turn) => turn.identity.turnId === activeTurn?.identity.turnId) &&
    !(streamState?.activeTurnId === activeTurn?.identity.turnId && !streamState?.isStreaming);
  // History owns completed Turns; the subscription only fills the unfinished tail.
  const shouldSubscribeTimeline =
    Boolean(streamState?.isStreaming && streamState.activeMessageId && !streamedTurnIsComplete) ||
    (sessionAvailable && sessionHistoryQuery.isSuccess) ||
    (sessionHistoryQuery.isSuccess && hasRemoteActiveTurn) ||
    observedMaintenanceSessionId === sessionId;
  const { retryTimeline } = useSessionTimelineEvents(
    sessionId,
    history?.lastStableEventId ?? undefined,
    Boolean(sessionId) && shouldSubscribeTimeline &&
      (!sessionAvailable || (sessionHistoryQuery.isSuccess && sessionHistoryQuery.isFetchedAfterMount)),
    async () => {
      await invalidateSessionQueries();
    },
    messageQueue,
    {
      stableEventSeq: history?.data.at(-1)?.completedEventSeq ?? 0,
      reloadHistory: async () => {
        const result = await sessionHistoryQuery.refetch();
        if (result.error) throw result.error;
        return result.data!;
      },
    },
  );
  const latestCompaction = latestContextCompaction(streamState);
  const contextCompaction = activeContextCompaction(streamState) ??
    (latestCompaction && !isContextCompactionRunning(latestCompaction)
      ? latestCompaction
      : undefined);
  const setCompactionObservationActive = useCallback((active: boolean) => {
    setObservedMaintenanceSessionId((observedSessionId) => {
      if (active) {
        return sessionId;
      }
      return observedSessionId === sessionId ? undefined : observedSessionId;
    });
  }, [sessionId]);

  return {
    contextCompaction,
    retryTimeline,
    setCompactionObservationActive,
  };
}

export function isLiveRecoverableActiveTurn(status: string | undefined) {
  return status === "running" || status === "waitingForInput" || status === "cancelling";
}
