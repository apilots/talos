import { apiErrorMessage, userVisibleError } from "@/shared/i18n/apiErrorMessage";
import { useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";

interface SidebarSessionDeleteDialogProps {
  deleting: boolean;
  sessionTitle: string;
  onClose: () => void;
  onDelete: () => Promise<void> | void;
}

export function SidebarSessionDeleteDialog({
  deleting,
  sessionTitle,
  onClose,
  onDelete,
}: SidebarSessionDeleteDialogProps) {
  const { t } = useTranslation(["sidebar", "common"]);
  const [error, setError] = useState<ReturnType<typeof userVisibleError> | null>(null);

  async function submitDelete() {
    if (deleting) return;
    setError(null);
    try {
      await onDelete();
    } catch (submitError) {
      setError(userVisibleError(submitError, "Failed to delete the session."));
    }
  }

  return createPortal(
    <div className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/30 px-4 backdrop-blur-[2px]">
      <div className="absolute inset-0" onClick={onClose} role="presentation" />
      <div className="relative z-10 w-[min(520px,calc(100vw-32px))] rounded-[var(--radius-panel)] border border-border bg-card p-6 shadow-panel">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-xl font-semibold tracking-normal text-ink">
              {t("deleteSessionTitle", { title: sessionTitle })}
            </h2>
            <p className="mt-3 max-w-[27rem] text-base leading-7 text-muted">
              {t("deleteSessionDescription")}
            </p>
          </div>
          <button
            aria-label={t("common:close")}
            className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-background hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
            onClick={onClose}
            type="button"
          >
            <X aria-hidden="true" size={18} />
          </button>
        </div>
        {error ? (
          <div className="mt-4 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
            {apiErrorMessage(error)}
          </div>
        ) : null}
        <div className="mt-7 flex justify-end gap-3">
          <Button className="min-w-24" onClick={onClose} type="button" variant="ghost">
            {t("common:cancel")}
          </Button>
          <Button
            className="min-w-24 bg-danger/12 text-danger hover:bg-danger/20"
            disabled={deleting}
            onClick={() => void submitDelete()}
            type="button"
          >
            {deleting ? t("deletingSession") : t("common:delete")}
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
