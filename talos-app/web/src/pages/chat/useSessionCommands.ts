import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiErrorMessage, userVisibleError } from "@/shared/i18n/apiErrorMessage";

import {
  isContextCompactionRunning,
  type ChatContextCompactionItem,
  type ComposerCommandResponseState,
  type ComposerSessionCommandId,
  useSessionActions,
} from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import type { ReasoningLevel } from "@/shared/types/ui";

interface UseSessionCommandsOptions {
  contextCompaction?: ChatContextCompactionItem;
  onCompactionObservationChange?: (active: boolean) => void;
  sessionId: string;
}

interface SessionCommandResponse {
  response: ComposerCommandResponseState;
  sessionId: string;
}

interface ManualCompactionOperation {
  itemId?: string;
  previousItemId?: string;
  sessionId: string;
  stopObservation: () => void;
}

export function useSessionCommands({
  contextCompaction,
  onCompactionObservationChange,
  sessionId,
}: UseSessionCommandsOptions) {
  useTranslation("chat");
  const [settledResponse, setSettledResponse] = useState<SessionCommandResponse>();
  const [clearConfirmationOpen, setClearConfirmationOpen] = useState(false);
  const [clearError, setClearError] = useState<ReturnType<typeof userVisibleError>>();
  const [manualCompactionStartingSessionId, setManualCompactionStartingSessionId] = useState<string>();
  const manualCompactionRef = useRef<ManualCompactionOperation | undefined>(undefined);
  const actions = useSessionActions(sessionId);
  const activeManualCompaction = contextCompaction?.trigger === "user_requested" &&
    isContextCompactionRunning(contextCompaction)
    ? contextCompaction
    : undefined;
  const response: ComposerCommandResponseState | undefined = activeManualCompaction ||
    manualCompactionStartingSessionId === sessionId
    ? {
        kind: "compact",
        stage: activeManualCompaction?.stage,
        state: "running",
      }
    : settledResponse?.sessionId === sessionId
      ? settledResponse.response
      : undefined;

  useEffect(() => () => {
    const operation = manualCompactionRef.current;
    if (operation?.sessionId !== sessionId) {
      return;
    }
    manualCompactionRef.current = undefined;
    setManualCompactionStartingSessionId((startingSessionId) =>
      startingSessionId === sessionId ? undefined : startingSessionId
    );
    operation.stopObservation();
  }, [sessionId]);

  useEffect(() => {
    if (!contextCompaction || contextCompaction.trigger !== "user_requested") {
      return;
    }
    const trackedCompaction = manualCompactionRef.current;
    if (isContextCompactionRunning(contextCompaction)) {
      if (trackedCompaction?.sessionId === sessionId) {
        if (
          trackedCompaction.itemId &&
          trackedCompaction.itemId !== contextCompaction.itemId
        ) {
          return;
        }
        if (
          !trackedCompaction.itemId &&
          trackedCompaction.previousItemId === contextCompaction.itemId
        ) {
          return;
        }
        trackedCompaction.itemId = contextCompaction.itemId;
      } else {
        manualCompactionRef.current = {
          itemId: contextCompaction.itemId,
          sessionId,
          stopObservation: () => undefined,
        };
      }
      setManualCompactionStartingSessionId((startingSessionId) =>
        startingSessionId === sessionId ? undefined : startingSessionId
      );
      return;
    }
    if (!trackedCompaction || trackedCompaction.sessionId !== sessionId) {
      return;
    }
    if (
      trackedCompaction.itemId
        ? trackedCompaction.itemId !== contextCompaction.itemId
        : trackedCompaction.previousItemId === contextCompaction.itemId
    ) {
      return;
    }
    manualCompactionRef.current = undefined;
    setManualCompactionStartingSessionId((startingSessionId) =>
      startingSessionId === sessionId ? undefined : startingSessionId
    );
    setSettledResponse({
      response: manualCompactionResult(contextCompaction),
      sessionId,
    });
    trackedCompaction.stopObservation();
  }, [contextCompaction, sessionId]);

  useEffect(() => {
    if (!response || response.kind === "status" || response.state !== "success") {
      return;
    }
    const timeout = window.setTimeout(
      () => setSettledResponse((current) =>
        current?.sessionId === sessionId ? undefined : current
      ),
      response.kind === "compact" ? 4_000 : 3_500,
    );
    return () => window.clearTimeout(timeout);
  }, [response, sessionId]);

  const execute = (command: ComposerSessionCommandId) => {
    if (contextCompaction && isContextCompactionRunning(contextCompaction)) {
      return;
    }
    setSettledResponse(undefined);
    if (command === "compact") {
      const operation: ManualCompactionOperation = {
        previousItemId: contextCompaction?.itemId,
        sessionId,
        stopObservation: () => onCompactionObservationChange?.(false),
      };
      manualCompactionRef.current = operation;
      setManualCompactionStartingSessionId(sessionId);
      onCompactionObservationChange?.(true);
      return actions.compact.mutateAsync()
        .then(() => undefined)
        .catch((error: unknown) => {
          if (manualCompactionRef.current !== operation) {
            throw error;
          }
          manualCompactionRef.current = undefined;
          operation.stopObservation();
          setManualCompactionStartingSessionId((startingSessionId) =>
            startingSessionId === sessionId ? undefined : startingSessionId
          );
          setSettledResponse({
            response: { kind: "compact", error: userVisibleError(error, "Failed to compact the session."), state: "error" },
            sessionId,
          });
          throw error;
        });
    }
    if (command === "clear") {
      setClearError(undefined);
      setClearConfirmationOpen(true);
      return;
    }
    setSettledResponse({ response: { kind: "status" }, sessionId });
  };

  const confirmClear = async () => {
    if (actions.clear.isPending) {
      return;
    }
    setClearError(undefined);
    try {
      await actions.clear.mutateAsync();
      useAppStore.getState().resetStream(sessionId);
      setClearConfirmationOpen(false);
      setSettledResponse({ response: { kind: "clear", state: "success" }, sessionId });
    } catch (error) {
      const failure = userVisibleError(error, "Failed to clear the session.");
      setClearError(failure);
      setSettledResponse({
        response: { kind: "clear", error: failure, state: "error" },
        sessionId,
      });
    }
  };

  return {
    cancelClear: () => {
      setClearConfirmationOpen(false);
      setClearError(undefined);
    },
    clearConfirmationOpen,
    clearError: clearError ? apiErrorMessage(clearError) : undefined,
    clearing: actions.clear.isPending,
    compactionStage: contextCompaction && isContextCompactionRunning(contextCompaction)
      ? contextCompaction.stage
      : undefined,
    confirmClear,
    dismissResponse: () => setSettledResponse((current) =>
      current?.sessionId === sessionId ? undefined : current
    ),
    execute,
    response,
    showModelChanged: (model: string, reasoningLevel?: ReasoningLevel) => {
      setSettledResponse({
        response: { kind: "model", model, reasoningLevel, state: "success" },
        sessionId,
      });
    },
    showModelChangeFailed: (model: string, error: unknown, reasoningLevel?: ReasoningLevel) => {
      setSettledResponse({
        response: { kind: "model", error: userVisibleError(error, "Failed to change the model."), model, reasoningLevel, state: "error" },
        sessionId,
      });
    },
  };
}

function manualCompactionResult(
  item: ChatContextCompactionItem,
): Extract<ComposerCommandResponseState, { kind: "compact"; state: "success" | "error" | "cancelled" }> {
  if (item.status === "cancelled") {
    return { kind: "compact", outcome: item.outcome ?? undefined, state: "cancelled" };
  }
  const failed = item.status === "failed" || item.stage === "failed" ||
    item.outcome === "rejected_missing_summary" || item.outcome?.startsWith("failed_");
  return {
    kind: "compact",
    outcome: item.outcome ?? undefined,
    state: failed ? "error" : "success",
  };
}
