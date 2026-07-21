import { ArrowUp, LoaderCircle, Square } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";

interface ComposerSubmitButtonProps {
  disabled?: boolean;
  draft: string;
  isBusy: boolean;
  isCancelling: boolean;
  onCancel?: () => void;
  onSend: () => Promise<void> | void;
}

export function ComposerSubmitButton({
  disabled,
  draft,
  isBusy,
  isCancelling,
  onCancel,
  onSend,
}: ComposerSubmitButtonProps) {
  const { t } = useTranslation(["chat", "common"]);
  const label = isBusy ? t("common:stopResponse") : t("chat:send");

  return (
    <button
      aria-label={label}
      className={cn(
        "flex h-8 w-8 shrink-0 items-center justify-center rounded-full outline-none transition focus:outline-none",
        isBusy
          ? "bg-danger text-white hover:bg-danger/90"
          : draft.trim().length === 0 || disabled
            ? "bg-muted/20 text-muted/60"
            : "bg-ink text-background hover:bg-ink/85",
      )}
      data-smoke="chat-composer-send"
      disabled={isBusy ? false : Boolean(disabled || draft.trim().length === 0)}
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
      {isCancelling ? (
        <LoaderCircle className="animate-spin" size={14} />
      ) : isBusy ? (
        <Square size={14} />
      ) : (
        <ArrowUp size={15} strokeWidth={2.5} />
      )}
    </button>
  );
}
