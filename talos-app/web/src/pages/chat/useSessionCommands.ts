import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  isActiveTurnRunningError,
  type ComposerCommandResponseState,
  type ComposerSessionCommandId,
  useSessionActions,
} from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import type { ContextCompactionStage } from "@/shared/types/protocol";
import type { ReasoningLevel } from "@/shared/types/ui";

interface UseSessionCommandsOptions {
  contextCompactionStage?: ContextCompactionStage;
  sessionId: string;
}

export function useSessionCommands({
  contextCompactionStage,
  sessionId,
}: UseSessionCommandsOptions) {
  const { t } = useTranslation("chat");
  const [settledResponse, setSettledResponse] = useState<ComposerCommandResponseState>();
  const [clearConfirmationOpen, setClearConfirmationOpen] = useState(false);
  const [clearError, setClearError] = useState<string>();
  const actions = useSessionActions(sessionId);
  const response: ComposerCommandResponseState | undefined = contextCompactionStage || actions.compact.isPending
    ? {
        kind: "compact",
        stage: contextCompactionStage,
        state: "running",
      }
    : settledResponse;

  useEffect(() => {
    if (!response || response.kind === "status" || response.state !== "success") {
      return;
    }
    const timeout = window.setTimeout(
      () => setSettledResponse(undefined),
      response.kind === "compact" ? 5_000 : 3_500,
    );
    return () => window.clearTimeout(timeout);
  }, [response]);

  const execute = (command: ComposerSessionCommandId) => {
    if (contextCompactionStage) {
      return;
    }
    setSettledResponse(undefined);
    if (command === "compact") {
      return actions.compact.mutateAsync()
        .then(() => setSettledResponse({ kind: "compact", state: "success" }))
        .catch((error: unknown) => {
          const message = isActiveTurnRunningError(error)
            ? t("contextCompactingWait")
            : t("commandCompactErrorDetail");
          setSettledResponse({ kind: "compact", message, state: "error" });
          throw error;
        });
    }
    if (command === "clear") {
      setClearError(undefined);
      setClearConfirmationOpen(true);
      return;
    }
    setSettledResponse({ kind: "status" });
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
      setSettledResponse({ kind: "clear", state: "success" });
    } catch (error) {
      const message = isActiveTurnRunningError(error)
        ? t("contextCompactingWait")
        : t("clearSessionFailed");
      setClearError(message);
      setSettledResponse({ kind: "clear", message, state: "error" });
    }
  };

  return {
    cancelClear: () => {
      setClearConfirmationOpen(false);
      setClearError(undefined);
    },
    clearConfirmationOpen,
    clearError,
    clearing: actions.clear.isPending,
    compactionStage: contextCompactionStage,
    confirmClear,
    dismissResponse: () => setSettledResponse(undefined),
    execute,
    response,
    showModelChanged: (model: string, reasoningLevel?: ReasoningLevel) => {
      setSettledResponse({ kind: "model", model, reasoningLevel, state: "success" });
    },
    showModelChangeFailed: (model: string, message: string, reasoningLevel?: ReasoningLevel) => {
      setSettledResponse({ kind: "model", message, model, reasoningLevel, state: "error" });
    },
  };
}
