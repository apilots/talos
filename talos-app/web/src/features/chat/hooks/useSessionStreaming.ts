import { useEffect } from "react";
import { useMutation } from "@tanstack/react-query";

import { cancelActiveTurn, isActiveTurnRunningError } from "../api";
import { useSessionMessageStream } from "./useSessionMessageStream";
import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import { useAppStore } from "@/shared/store/app-store";
import type { ChatUserInput } from "@/shared/types/ui";
import type { StartTurnRequest } from "@talos/sdk-browser";

export function useSessionStreaming(
  sessionId: string,
  serverMessageCount?: number,
  sessionTitle?: string,
) {
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const setSessionStatus = useAppStore((state) => state.setSessionStatus);
  const resetStream = useAppStore((state) => state.resetStream);
  const setPendingInput = useAppStore((state) => state.setPendingInput);
  const { eventMessage, cancelStream, retryMessage } = useSessionMessageStream(sessionId);
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
      request,
    }: {
      text: string;
      appendUserInput?: boolean;
      request?: StartTurnRequest;
    }) => {
      const clientMessageId = createClientMessageId();
      const input = turnUserInput(text, request);
      resetStream(sessionId);
      if (appendUserInput && hasUserInput(input)) {
        setPendingInput(sessionId, { ...input, clientMessageId, phase: "local" });
      }
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "awaiting",
        activeInput: appendUserInput ? input : undefined,
        activeClientMessageId: appendUserInput ? clientMessageId : undefined,
        awaitingFinalResponse: false,
      });
      setSessionStatus(sessionId, "running");
      await eventMessage(request ?? { session_id: sessionId, input: [{ type: "text", text }] }, (messageId) => {
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
      if (isActiveTurnRunningError(error)) {
        useAppStore.getState().setPendingInput(sessionId, undefined);
        useAppStore.getState().setStreamError(sessionId, "Context compacting... Please wait.");
        useAppStore.getState().setStreamMeta(sessionId, {
          isStreaming: false,
          connectionState: "idle",
          activeMessageId: undefined,
          awaitingFinalResponse: false,
        });
        return;
      }
      const errorName = error instanceof Error ? error.name : "";
      const isAbort = errorName === "AbortError";
      const isCancelling = currentStream?.connectionState === "cancelling";
      if (isCancelling) {
        return;
      }

      if (isAbort || (currentStream?.isStreaming && currentStream.activeMessageId)) {
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

      useAppStore.getState().setPendingInput(sessionId, undefined);
      useAppStore.getState().setStreamError(
        sessionId,
        error instanceof Error ? error.message : "Failed to stream message.",
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

  const retryMutation = useMutation({
    mutationFn: async () => {
      const currentStream = useAppStore.getState().streams[sessionId];
      const input = currentStream?.activeInput;
      const retryOfMessageId = currentStream?.activeMessageId;
      const clientMessageId = currentStream?.activeClientMessageId ?? createClientMessageId();
      if (!input || !hasUserInput(input) || !retryOfMessageId) {
        throw new Error("No retryable message attempt.");
      }

      resetStream(sessionId);
      setPendingInput(sessionId, { ...input, clientMessageId, phase: "local" });
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "awaiting",
        activeInput: input,
        activeClientMessageId: clientMessageId,
        awaitingFinalResponse: false,
      });
      setSessionStatus(sessionId, "running");
      await retryMessage({
        clientMessageId,
        retryOfMessageId,
        input,
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
      useAppStore.getState().setPendingInput(sessionId, undefined);
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
    canRetryResponse: Boolean(
      streamState?.activeInput &&
        hasUserInput(streamState.activeInput) &&
        streamState.activeMessageId,
    ),
    sendText: (text: string, request?: StartTurnRequest) => {
      const content = text.trim();
      const hasFiles = request?.input.some(
        (item) => item.type === "files" && item.fileIds.length > 0,
      );
      if (!content && !hasFiles) {
        return Promise.resolve();
      }
      return sendMutation.mutateAsync({ text: content, appendUserInput: true, request });
    },
    isStreaming: streamState?.isStreaming ?? false,
    isCancelling:
      stopMutation.isPending ||
      retryMutation.isPending ||
      streamState?.connectionState === "cancelling",
  };
}

function turnUserInput(text: string, request?: StartTurnRequest): ChatUserInput {
  const fileIds =
    request?.input.flatMap((item) => (item.type === "files" ? item.fileIds : [])) ?? [];
  return { text, fileIds };
}

function hasUserInput(input: ChatUserInput) {
  return Boolean(input.text.trim() || input.fileIds.length > 0);
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
