import { ArrowUp, Square } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";

interface ComposerSubmitButtonProps {
  disabled?: boolean;
  canSubmit: boolean;
  isBusy: boolean;
  isCancelling: boolean;
  onCancel?: () => void;
  onSend: () => Promise<void> | void;
}

export function ComposerSubmitButton({
  disabled,
  canSubmit,
  isBusy,
  isCancelling,
  onCancel,
  onSend,
}: ComposerSubmitButtonProps) {
  const { t } = useTranslation(["chat", "common"]);
  const label = isBusy ? t("common:stopResponse") : t("chat:send");
  const state = isCancelling
    ? "cancelling"
    : isBusy
      ? "running"
      : disabled || !canSubmit
        ? "disabled"
        : "ready";

  return (
    <button
      aria-label={label}
      className={cn(
        "flex h-9 w-9 shrink-0 items-center justify-center rounded-full outline-none transition focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/25",
        isCancelling && "animate-pulse",
        isBusy
          ? "bg-ink text-background hover:bg-actionHover"
          : !canSubmit || disabled
            ? "cursor-not-allowed bg-muted/20 text-muted/45"
            : "bg-brand text-background hover:bg-actionHover",
      )}
      data-smoke="chat-composer-send"
      data-state={state}
      disabled={isCancelling || (!isBusy && Boolean(disabled || !canSubmit))}
      onClick={() => {
        if (isBusy) {
          onCancel?.();
          return;
        }
        void onSend();
      }}
      title={label}
      type="button"
    >
      {isBusy ? (
        <Square fill="currentColor" size={11} strokeWidth={1.5} />
      ) : (
        <ArrowUp size={15} strokeWidth={2.5} />
      )}
    </button>
  );
}
