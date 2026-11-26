import { AlertCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { isServiceUnavailable } from "@/shared/api/is-service-unavailable";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

/** Directory failures are not evidence that a Computer no longer exists. */
export function ComputerDirectoryError({ error, onRetry, pending, compact = false }: {
  error: unknown;
  onRetry: () => void;
  pending?: boolean;
  compact?: boolean;
}) {
  const { t } = useTranslation(["computers", "common"]);
  const message = isServiceUnavailable(error)
    ? t("computers:networkError")
    : userVisibleErrorMessage(error, t("computers:loadError"));
  return (
    <div role="alert" className={compact ? "text-danger" : "cmp-notice text-sm text-danger"}>
      <span className={compact ? "sr-only" : undefined}>{message}</span>
      <button
        type="button"
        title={compact ? message : undefined}
        aria-label={compact ? `${message} ${t("common:retryNow")}` : undefined}
        disabled={pending}
        onClick={onRetry}
        className={compact ? "flex size-10 items-center justify-center rounded-xl hover:bg-hover focus-visible:outline focus-visible:outline-2" : "ml-2 underline disabled:opacity-50"}
      >
        {compact ? <AlertCircle size={20} aria-hidden="true" /> : t("common:retryNow")}
      </button>
    </div>
  );
}
