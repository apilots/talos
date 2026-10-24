import { overlayClasses } from "@/shared/ui/overlayStyles";
import { useEffect } from "react";
import { createPortal } from "react-dom";
import { Eraser, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";

interface SessionClearConfirmationDialogProps {
  error?: string;
  clearing: boolean;
  onCancel: () => void;
  onConfirm: () => Promise<void> | void;
  open: boolean;
}

/** Confirms destructive session clearing while preserving the unsent Composer draft. */
export function SessionClearConfirmationDialog({
  error,
  clearing,
  onCancel,
  onConfirm,
  open,
}: SessionClearConfirmationDialogProps) {
  const { t } = useTranslation(["chat", "common"]);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !clearing) {
        onCancel();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [clearing, onCancel, open]);

  if (!open) {
    return null;
  }

  return createPortal(
    <div className={`fixed inset-0 z-[1000] flex items-center justify-center px-4 ${overlayClasses.tint}`}>
      <div className="absolute inset-0" onClick={clearing ? undefined : onCancel} role="presentation" />
      <div
        aria-labelledby="clear-session-title"
        aria-modal="true"
        className="relative z-10 w-[min(440px,calc(100vw-32px))] rounded-[var(--radius-panel)] border border-border bg-card p-5 shadow-panel"
        role="alertdialog"
      >
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="mb-4 flex size-9 items-center justify-center rounded-[var(--radius-control)] border border-danger/20 bg-danger/[0.08] text-danger">
              <Eraser aria-hidden="true" size={17} />
            </div>
            <h2 className="text-lg font-semibold text-ink" id="clear-session-title">
              {t("chat:clearSessionConfirmTitle")}
            </h2>
            <p className="mt-2 text-[13px] leading-5 text-muted">
              {t("chat:clearSessionConfirmDescription")}
            </p>
          </div>
          <button
            aria-label={t("common:close")}
            className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-hover hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
            disabled={clearing}
            onClick={onCancel}
            type="button"
          >
            <X aria-hidden="true" size={16} />
          </button>
        </div>

        <div className="mt-4 rounded-[var(--radius-control)] border border-border/70 bg-surface/55 px-3 py-2.5 text-[12px] leading-5 text-muted">
          {t("chat:clearSessionDraftPreserved")}
        </div>
        {error ? (
          <p className="mt-3 rounded-[var(--radius-control)] border border-danger/25 bg-danger/[0.08] px-3 py-2 text-[12px] text-danger" role="alert">
            {error}
          </p>
        ) : null}
        <div className="mt-5 flex justify-end gap-2">
          <Button autoFocus disabled={clearing} onClick={onCancel} type="button" variant="ghost">
            {t("common:cancel")}
          </Button>
          <Button disabled={clearing} onClick={() => void onConfirm()} type="button" variant="danger">
            {t("chat:clearSessionConfirmAction")}
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
