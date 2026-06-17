import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

interface ChatSessionReconnectPanelProps {
  canRetryResponse: boolean;
  isCancelling: boolean;
  onRetryResponse: () => void;
  onRetryStream: () => void;
  onStopResponse: () => void;
}

export function ChatSessionReconnectPanel({
  canRetryResponse,
  isCancelling,
  onRetryResponse,
  onRetryStream,
  onStopResponse,
}: ChatSessionReconnectPanelProps) {
  const { t } = useTranslation(["chat", "common"]);

  return (
    <Panel className="rounded-xl border-border/70 bg-surface/70 px-3 py-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0 text-sm text-muted">
          <span className="font-medium text-ink">{t("chat:liveResponseInterrupted")}</span>
          <span className="ml-2">{t("chat:interruptedDetail")}</span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            className="px-2.5 py-1.5 text-xs"
            disabled={isCancelling}
            onClick={onRetryStream}
            variant="secondary"
          >
            {t("common:refreshNow")}
          </Button>
          {canRetryResponse ? (
            <Button
              className="px-2.5 py-1.5 text-xs"
              disabled={isCancelling}
              onClick={onRetryResponse}
              variant="secondary"
            >
              {t("common:retryResponse")}
            </Button>
          ) : null}
          <Button
            className="px-2.5 py-1.5 text-xs"
            disabled={isCancelling}
            onClick={onStopResponse}
            variant="ghost"
          >
            {t("common:stopResponse")}
          </Button>
        </div>
      </div>
    </Panel>
  );
}
