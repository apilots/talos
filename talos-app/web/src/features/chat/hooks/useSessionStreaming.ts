import { useEffect } from "react";
import { useMutation } from "@tanstack/react-query";

import { cancelActiveTurn } from "../api";
import { useSessionMessageStream } from "./useSessionMessageStream";
import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import { useAppStore } from "@/shared/store/app-store";

export function useSessionStreaming(sessionId: string, serverMessageCount?: number) {
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const setSessionStatus = useAppStore((state) => state.setSessionStatus);
  const resetStream = useAppStore((state) => state.resetStream);
  const appendStreamUserInput = useAppStore((state) => state.appendStreamUserInput);
  const { streamMessage, cancelStream, hasActiveConnection } = useSessionMessageStream(sessionId);
  const { invalidateSessionQueries } = useSessionQueryInvalidation(sessionId);

  useEffect(() => {
    return () => {
      cancelStream();
    };
  }, [cancelStream]);

  const stopMutation = useMutation({
    mutationFn: async () => {
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "cancelling",
      });
      await cancelActiveTurn(sessionId);
      cancelStream();
    },
    onSuccess: async () => {
      useAppStore.getState().setPendingApproval(sessionId, undefined);
      useAppStore.getState().setSessionStatus(sessionId, undefined);
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        expectedMessageCount: undefined,
      });
      await invalidateSessionQueries();
    },
    onError: (error) => {
      useAppStore.getState().setStreamError(
        sessionId,
        error instanceof Error ? error.message : "Failed to stop response.",
      );
      useAppStore.getState().setSessionStatus(sessionId, "error");
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
      });
    },
  });

  const sendMutation = useMutation({
    mutationFn: async (text: string) => {
      resetStream(sessionId);
      appendStreamUserInput(sessionId, text);
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "awaiting",
        expectedMessageCount:
          typeof serverMessageCount === "number" ? serverMessageCount + 1 : undefined,
      });
      setSessionStatus(sessionId, "running");
      await streamMessage(text);
    },
    onSuccess: async () => {
      await invalidateSessionQueries();
      resetStream(sessionId);
      setSessionStatus(sessionId, undefined);
    },
    onError: async (error) => {
      const currentStream = useAppStore.getState().streams[sessionId];
      const errorName = error instanceof Error ? error.name : "";
      const isAbort = errorName === "AbortError";
      const isCancelling = currentStream?.connectionState === "cancelling";
      if (isCancelling) {
        return;
      }

      if (isAbort || currentStream?.isStreaming) {
        useAppStore.getState().setStreamError(sessionId, undefined);
        useAppStore.getState().setStreamMeta(sessionId, {
          isStreaming: true,
          connectionState: "detached",
        });
        setSessionStatus(sessionId, "running");
        await invalidateSessionQueries();
        return;
      }

      useAppStore.getState().setStreamError(
        sessionId,
        error instanceof Error ? error.message : "Failed to stream message.",
      );
      setSessionStatus(sessionId, "error");
      await invalidateSessionQueries();
    },
  });

  return {
    hasActiveConnection,
    sendMutation,
    stopMutation,
    streamState,
    stopResponse: () => stopMutation.mutateAsync(),
    sendText: (text: string) => {
      const content = text.trim();
      if (!content) {
        return Promise.resolve();
      }
      return sendMutation.mutateAsync(content);
    },
    isStreaming: streamState?.isStreaming ?? false,
    isCancelling: stopMutation.isPending || streamState?.connectionState === "cancelling",
  };
}
