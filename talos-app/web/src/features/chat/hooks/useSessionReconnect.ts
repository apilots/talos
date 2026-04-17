import { useEffect } from "react";

import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import { useAppStore } from "@/shared/store/app-store";
import type { StreamSessionState } from "@/shared/types/ui";

export function useSessionReconnect({
  hasActiveConnection,
  serverMessageCount,
  sessionId,
  streamState,
}: {
  hasActiveConnection: () => boolean;
  serverMessageCount?: number;
  sessionId: string;
  streamState?: StreamSessionState;
}) {
  const resetStream = useAppStore((state) => state.resetStream);
  const { connectionKey, invalidateSessionQueries, queryClient } = useSessionQueryInvalidation(sessionId);

  useEffect(() => {
    if (!streamState?.isStreaming || streamState.pendingApproval) {
      return;
    }

    const timeoutId = window.setTimeout(() => {
      const currentStream = useAppStore.getState().streams[sessionId];
      if (!currentStream?.isStreaming || currentStream.pendingApproval) {
        return;
      }
      if (currentStream.connectionState === "cancelling") {
        return;
      }
      if (hasActiveConnection()) {
        return;
      }
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "detached",
      });
      useAppStore.getState().setSessionStatus(sessionId, "running");
    }, 350);

    return () => window.clearTimeout(timeoutId);
  }, [
    hasActiveConnection,
    sessionId,
    streamState?.connectionState,
    streamState?.isStreaming,
    streamState?.pendingApproval,
  ]);

  useEffect(() => {
    if (
      !streamState?.isStreaming ||
      typeof streamState.expectedMessageCount !== "number" ||
      typeof serverMessageCount !== "number" ||
      serverMessageCount < streamState.expectedMessageCount
    ) {
      return;
    }

    void queryClient
      .invalidateQueries({ queryKey: ["talos", connectionKey, "turns", sessionId] })
      .then(() => {
        useAppStore.getState().setPendingApproval(sessionId, undefined);
        useAppStore.getState().setSessionStatus(sessionId, undefined);
        resetStream(sessionId);
      });
  }, [
    connectionKey,
    queryClient,
    resetStream,
    serverMessageCount,
    sessionId,
    streamState?.expectedMessageCount,
    streamState?.isStreaming,
  ]);

  useEffect(() => {
    if (
      !streamState?.isStreaming ||
      !(streamState.connectionState === "detached" || streamState.connectionState === "reconnecting")
    ) {
      return;
    }

    let disposed = false;
    const refresh = async () => {
      await invalidateSessionQueries();

      if (disposed) {
        return;
      }

      const currentStream = useAppStore.getState().streams[sessionId];
      if (!currentStream?.isStreaming) {
        return;
      }
      if (currentStream.connectionState === "reconnecting") {
        useAppStore.getState().setStreamMeta(sessionId, {
          connectionState: "detached",
        });
      }
    };

    void refresh();
    const intervalId = window.setInterval(() => {
      void refresh();
    }, 4_000);

    return () => {
      disposed = true;
      window.clearInterval(intervalId);
    };
  }, [invalidateSessionQueries, sessionId, streamState?.connectionState, streamState?.isStreaming]);

  return {
    retryStream: async () => {
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setSessionStatus(sessionId, "running");
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "reconnecting",
      });
      await invalidateSessionQueries();
    },
  };
}
