import { useMutation } from "@tanstack/react-query";

import { cancelActiveTurn, isActiveTurnRunningError, retryTimelineTurn, startTimelineTurn } from "../api";
import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useAppStore } from "@/shared/store/app-store";
import type { ChatUserInput } from "@/shared/types/ui";
import type { StartTurnRequest } from "@talos/sdk-browser";

export function useSessionStreaming(
  sessionId: string,
) {
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const setSessionStatus = useAppStore((state) => state.setSessionStatus);
  const resetStream = useAppStore((state) => state.resetStream);
  const setPendingInput = useAppStore((state) => state.setPendingInput);
  const { invalidateSessionQueries } =
    useSessionQueryInvalidation(sessionId);

  const stopMutation = useMutation({
    mutationFn: async (turnId: string) => {
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "cancelling",
      });
      await cancelActiveTurn(sessionId, turnId);
    },
    onSuccess: async () => {
      await invalidateSessionQueries();
    },
    onError: (error) => {
      useAppStore.getState().setStreamError(
        sessionId,
        userVisibleErrorMessage(error, "Failed to stop response."),
      );
      useAppStore.getState().setSessionStatus(sessionId, "error");
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
      });
    },
  });

  const sendMutation = useMutation({
    mutationFn: async ({
      text,
      request,
    }: {
      text: string;
      request?: StartTurnRequest;
    }) => {
      const clientMessageId = createClientMessageId();
      const input = turnUserInput(text, request);
      const lastEventId = useAppStore.getState().streams[sessionId]?.lastEventId;
      const lastEventSeq = useAppStore.getState().streams[sessionId]?.lastEventSeq;
      resetStream(sessionId);
      if (hasUserInput(input)) {
        setPendingInput(sessionId, { ...input, clientMessageId, phase: "local" });
      }
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "awaiting",
        lastEventId,
        lastEventSeq,
        activeInput: input,
        activeClientMessageId: clientMessageId,
      });
      setSessionStatus(sessionId, "running");
      await startTimelineTurn(request ?? {
        sessionId,
        input: { content: [{ type: "text", text }] },
        modelSelection: null,
        userPermissionMode: null,
        projectId: null,
        currentDirectory: null,
        clientRequestId: null,
      }, (messageId) => {
        markPendingInputAccepted(sessionId, clientMessageId, messageId);
      });
    },
    onError: async (error) => {
      const currentStream = useAppStore.getState().streams[sessionId];
      if (isActiveTurnRunningError(error)) {
        useAppStore.getState().setPendingInput(sessionId, undefined);
        useAppStore.getState().setStreamError(
          sessionId,
          userVisibleErrorMessage(error, "A turn is already running."),
        );
        useAppStore.getState().setStreamMeta(sessionId, {
          isStreaming: false,
          connectionState: "idle",
          activeMessageId: undefined,
        });
        await invalidateSessionQueries();
        return;
      }
      const isCancelling = currentStream?.connectionState === "cancelling";
      if (isCancelling) {
        return;
      }

      if (currentStream?.isStreaming && currentStream.activeMessageId) {
        useAppStore.getState().setStreamError(sessionId, undefined);
        useAppStore.getState().setStreamMeta(sessionId, {
          isStreaming: true,
          connectionState: "detached",
        });
        setSessionStatus(sessionId, "running");
        await invalidateSessionQueries();
        return;
      }

      useAppStore.getState().setPendingInput(sessionId, undefined);
      useAppStore.getState().setStreamError(
        sessionId,
        userVisibleErrorMessage(error, "Failed to stream message."),
      );
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
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
        lastEventId: currentStream?.lastEventId,
        lastEventSeq: currentStream?.lastEventSeq,
        activeInput: input,
        activeClientMessageId: clientMessageId,
      });
      setSessionStatus(sessionId, "running");
      await retryTimelineTurn({
        sessionId,
        clientMessageId,
        retryOfMessageId,
        input: {
          content: [
            ...(input.text.trim() ? [{ type: "text" as const, text: input.text }] : []),
            ...input.fileIds.map((fileId) => ({ type: "file" as const, fileId })),
          ],
        },
      }, (messageId) => {
        markPendingInputAccepted(sessionId, clientMessageId, messageId);
      });
    },
    onError: async (error) => {
      useAppStore.getState().setPendingInput(sessionId, undefined);
      useAppStore.getState().setStreamError(
        sessionId,
        userVisibleErrorMessage(error, "Failed to retry response."),
      );
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
      });
      setSessionStatus(sessionId, "error");
      await invalidateSessionQueries();
    },
  });

  return {
    sendMutation,
    stopMutation,
    streamState,
    stopResponse: (turnId: string) => {
      if (stopMutation.isPending) {
        return Promise.resolve();
      }
      return stopMutation.mutateAsync(turnId);
    },
    retryResponse: () => retryMutation.isPending ? Promise.resolve() : retryMutation.mutateAsync(),
    canRetryResponse: Boolean(
      !retryMutation.isPending && streamState?.activeInput &&
        hasUserInput(streamState.activeInput) &&
        streamState.activeMessageId,
    ),
    sendText: (text: string, request?: StartTurnRequest) => {
      const content = text.trim();
      const hasFiles = request?.input.content.some((item) => item.type === "file");
      if (!content && !hasFiles) {
        return Promise.resolve();
      }
      return sendMutation.mutateAsync({ text: content, request });
    },
    isStreaming: streamState?.isStreaming ?? false,
    isCancelling:
      stopMutation.isPending ||
      streamState?.connectionState === "cancelling",
  };
}

function turnUserInput(text: string, request?: StartTurnRequest): ChatUserInput {
  const fileIds =
    request?.input.content.flatMap((item) => (item.type === "file" ? [item.fileId] : [])) ?? [];
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
