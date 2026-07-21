import { useState } from "react";
import type { ComponentProps } from "react";
import { useTranslation } from "react-i18next";

import {
  ChatComposer,
  isActiveTurnRunningError,
  useSessionActions,
} from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import type { ContextCompactionStage } from "@/shared/types/protocol";

interface UseSessionCommandsOptions {
  contextCompactionStage?: ContextCompactionStage;
  currentModel?: string;
  isStreaming: boolean;
  sessionId: string;
}

type ComposerCommand = Parameters<
  NonNullable<ComponentProps<typeof ChatComposer>["onCommand"]>
>[0];

export function useSessionCommands({
  contextCompactionStage,
  currentModel,
  isStreaming,
  sessionId,
}: UseSessionCommandsOptions) {
  const { t } = useTranslation("chat");
  const [notice, setNotice] = useState<string>();
  const actions = useSessionActions(sessionId);
  const compactionStage = contextCompactionStage;

  const execute = (command: ComposerCommand) => {
    if (compactionStage) {
      return;
    }
    setNotice(undefined);
    if (command === "compact") {
      return actions.compact.mutateAsync().then(() => undefined).catch(handleBusyError);
    }
    if (command === "clear") {
      return actions.clear.mutateAsync().then(() => {
        useAppStore.getState().resetStream(sessionId);
      }).catch(handleBusyError);
    }
    if (command === "status") {
      const status = isStreaming ? t("sessionStatusRunning") : t("sessionStatusReady");
      setNotice(t("sessionStatusSummary", {
        model: currentModel ?? t("sessionStatusUnknownModel"),
        status,
      }));
    }
  };

  function handleBusyError(error: unknown): never {
    if (isActiveTurnRunningError(error)) {
      setNotice(t("contextCompactingWait"));
    }
    throw error;
  }

  return { compactionStage, execute, notice };
}
