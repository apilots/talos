import { useEffect } from "react";
import { useMutation } from "@tanstack/react-query";

import { cancelActiveTurn } from "../api";
import { useSessionMessageStream } from "./useSessionMessageStream";
import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import { useAppStore } from "@/shared/store/app-store";

export function useSessionStreaming(
  sessionId: string,
  serverMessageCount?: number,
  sessionTitle?: string,
) {
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const setSessionStatus = useAppStore((state) => state.setSessionStatus);
  const resetStream = useAppStore((state) => state.resetStream);
  const setPendingInput = useAppStore((state) => state.setPendingInput);
  const { eventMessage, cancelStream, reconnectMessage, retryMessage, hasActiveConnection } =
    useSessionMessageStream(sessionId);
  const { invalidateSessionQueries, refreshSessionTitleAfterTurn } =
    useSessionQueryInvalidation(sessionId);

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
        awaitingFinalResponse: false,
      });
      cancelStream();
      await cancelActiveTurn(sessionId);
    },
    onSuccess: async () => {
      useAppStore.getState().setPendingUserAction(sessionId, undefined);
      useAppStore.getState().setSessionStatus(sessionId, undefined);
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        awaitingFinalResponse: false,
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
        awaitingFinalResponse: false,
      });
    },
  });

  const sendMutation = useMutation({
    mutationFn: async ({
      text,
      appendUserInput = true,
    }: {
      text: string;
      appendUserInput?: boolean;
    }) => {
      const clientMessageId = createClientMessageId();
      resetStream(sessionId);
      if (appendUserInput && text.trim().length > 0) {
        setPendingInput(sessionId, { text, clientMessageId, phase: "local" });
      }
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "awaiting",
        activeInputText: appendUserInput ? text : undefined,
        activeClientMessageId: appendUserInput ? clientMessageId : undefined,
        awaitingFinalResponse: false,
      });
      setSessionStatus(sessionId, "running");
      await eventMessage(text, (messageId) => {
        markPendingInputAccepted(sessionId, clientMessageId, messageId);
      });
    },
    onSuccess: async () => {
      await invalidateSessionQueries();
      const currentStream = useAppStore.getState().streams[sessionId];
      if (currentStream?.pendingUserAction) {
        setSessionStatus(sessionId, "running");
        return;
      }
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        awaitingFinalResponse: false,
      });
      setSessionStatus(sessionId, undefined);
      void refreshSessionTitleAfterTurn({
        serverMessageCount,
        title: sessionTitle,
      });
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
          awaitingFinalResponse: false,
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

  const retryMutation = useMutation({
    mutationFn: async () => {
      const currentStream = useAppStore.getState().streams[sessionId];
      const text = currentStream?.activeInputText?.trim();
      const retryOfMessageId = currentStream?.activeMessageId;
      const clientMessageId = currentStream?.activeClientMessageId ?? createClientMessageId();
      if (!text || !retryOfMessageId) {
        throw new Error("No retryable message attempt.");
      }

      resetStream(sessionId);
      setPendingInput(sessionId, { text, clientMessageId, phase: "local" });
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "awaiting",
        activeInputText: text,
        activeClientMessageId: clientMessageId,
        awaitingFinalResponse: false,
      });
      setSessionStatus(sessionId, "running");
      await retryMessage({
        clientMessageId,
        retryOfMessageId,
        text,
        onAcceptedMessageId: (messageId) => {
          markPendingInputAccepted(sessionId, clientMessageId, messageId);
        },
      });
    },
    onSuccess: async () => {
      await invalidateSessionQueries();
      const currentStream = useAppStore.getState().streams[sessionId];
      if (currentStream?.pendingUserAction) {
        setSessionStatus(sessionId, "running");
        return;
      }
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        awaitingFinalResponse: false,
      });
      setSessionStatus(sessionId, undefined);
      void refreshSessionTitleAfterTurn({
        serverMessageCount,
        title: sessionTitle,
      });
    },
    onError: async (error) => {
      useAppStore.getState().setStreamError(
        sessionId,
        error instanceof Error ? error.message : "Failed to retry response.",
      );
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        awaitingFinalResponse: false,
      });
      setSessionStatus(sessionId, "error");
      await invalidateSessionQueries();
    },
  });

  return {
    hasActiveConnection,
    reconnectMessage,
    sendMutation,
    stopMutation,
    streamState,
    stopResponse: () => {
      if (stopMutation.isPending) {
        return Promise.resolve();
      }
      return stopMutation.mutateAsync();
    },
    retryResponse: () => retryMutation.mutateAsync(),
    canRetryResponse: Boolean(streamState?.activeInputText && streamState.activeMessageId),
    sendText: (text: string) => {
      const content = text.trim();
      if (!content) {
        return Promise.resolve();
      }
      return sendMutation.mutateAsync({ text: content, appendUserInput: true });
    },
    isStreaming: streamState?.isStreaming ?? false,
    isCancelling:
      stopMutation.isPending ||
      retryMutation.isPending ||
      streamState?.connectionState === "cancelling",
  };
}

function createClientMessageId() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return `client_${globalThis.crypto.randomUUID()}`;
  }
  return `client_${Date.now()}_${Math.random().toString(36).slice(2)}`;
}

function markPendingInputAccepted(
  sessionId: string,
  clientMessageId: string,
  serverMessageId: string,
) {
  const currentPendingInput = useAppStore.getState().streams[sessionId]?.pendingInput;
  if (!currentPendingInput || currentPendingInput.clientMessageId !== clientMessageId) {
    return;
  }
  useAppStore.getState().setPendingInput(sessionId, {
    ...currentPendingInput,
    serverMessageId,
    phase: "accepted",
  });
  useAppStore.getState().setStreamMeta(sessionId, {
    activeMessageId: serverMessageId,
  });
}
