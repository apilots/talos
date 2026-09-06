import { useCallback, useState } from "react";

import {
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
  sessionHistoryQuery: ReturnType<typeof useSessionHistoryQuery>;
  sessionId: string;
  streamState?: StreamSessionState;
  messageQueue: ReturnType<typeof useSessionMessageQueue>;
}

/** Owns Session-scoped Timeline recovery, maintenance observation, and compaction selection. */
export function useChatSessionTimelineLifecycle({
  sessionHistoryQuery,
  sessionId,
  streamState,
  messageQueue,
}: UseChatSessionTimelineLifecycleOptions) {
  const [observedMaintenanceSessionId, setObservedMaintenanceSessionId] = useState<string>();
  const { invalidateSessionQueries } = useSessionQueryInvalidation(sessionId);
  // SSE owns paginated replay and live handoff. A second recovery query can
  // overwrite newer events and stop replay on an older Turn's terminal event.
  const shouldSubscribeTimeline =
    Boolean(streamState?.isStreaming && streamState.activeMessageId) ||
    sessionHistoryQuery.isSuccess ||
    observedMaintenanceSessionId === sessionId;
  const { retryTimeline } = useSessionTimelineEvents(
    sessionId,
    undefined,
    Boolean(sessionId) && shouldSubscribeTimeline,
    async () => {
      await invalidateSessionQueries();
    },
    messageQueue,
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
