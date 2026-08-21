import { useCallback, useEffect, useState } from "react";

import {
  type useActiveTurnQuery,
  useLiveRecoveryQuery,
  type useSessionHistoryQuery,
} from "./useChatQueries";
import { useSessionTimelineEvents } from "./useSessionTimelineEvents";
import { applyTimelineRecovery } from "../stream-events";
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
  streamState?: StreamSessionState;
}

/** Owns Session-scoped Timeline recovery, maintenance observation, and compaction selection. */
export function useChatSessionTimelineLifecycle({
  activeTurnQuery,
  sessionHistoryQuery,
  sessionId,
  streamState,
}: UseChatSessionTimelineLifecycleOptions) {
  const [observedMaintenanceSessionId, setObservedMaintenanceSessionId] = useState<string>();
  const shouldRecoverLiveTail =
    !streamState?.isStreaming &&
    isLiveRecoverableActiveTurn(activeTurnQuery.data?.status);
  const liveRecoveryQuery = useLiveRecoveryQuery(
    sessionId,
    sessionHistoryQuery.data?.last_stable_event_id ?? undefined,
    shouldRecoverLiveTail && sessionHistoryQuery.isSuccess,
  );
  const shouldSubscribeTimeline =
    Boolean(streamState?.isStreaming && streamState.activeMessageId) ||
    (shouldRecoverLiveTail && sessionHistoryQuery.isSuccess) ||
    observedMaintenanceSessionId === sessionId;
  const { retryTimeline } = useSessionTimelineEvents(
    sessionId,
    sessionHistoryQuery.data?.last_stable_event_id ?? undefined,
    Boolean(sessionId) && shouldSubscribeTimeline,
    async () => {
      await Promise.all([
        sessionHistoryQuery.refetch(),
        activeTurnQuery.refetch(),
      ]);
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

  useEffect(() => {
    if (liveRecoveryQuery.data) {
      applyTimelineRecovery(sessionId, liveRecoveryQuery.data);
    }
  }, [sessionId, liveRecoveryQuery.data]);

  return {
    contextCompaction,
    retryTimeline,
    setCompactionObservationActive,
  };
}

export function isLiveRecoverableActiveTurn(status: string | undefined) {
  return status === "running" || status === "waiting_user_action" || status === "cancelling";
}
