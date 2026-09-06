import { apiErrorMessage, userVisibleError } from "@/shared/i18n/apiErrorMessage";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

import { Button } from "@/shared/ui/Button";

interface SidebarProjectRenameDialogProps {
  initialName: string;
  saving: boolean;
  onClose: () => void;
  onSave: (name: string) => Promise<void> | void;
}

export function SidebarProjectRenameDialog({
  initialName,
  saving,
  onClose,
  onSave,
}: SidebarProjectRenameDialogProps) {
  const [name, setName] = useState(initialName);
  useTranslation("errors");
  const [error, setError] = useState<ReturnType<typeof userVisibleError> | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const trimmedName = name.trim();
  const canSave = trimmedName.length > 0 && trimmedName !== initialName;

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  async function submitRename() {
    if (!canSave || saving) {
      return;
    }
    setError(null);
    try {
      await onSave(trimmedName);
    } catch (submitError) {
      setError(userVisibleError(submitError, "Failed to rename the project."));
    }
  }

  return createPortal(
    <div className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/30 px-4 backdrop-blur-[2px]">
      <div className="absolute inset-0" onClick={onClose} role="presentation" />
      <form
        className="relative z-10 w-[min(560px,calc(100vw-32px))] rounded-[var(--radius-panel)] border border-border bg-card p-6 shadow-panel"
        onSubmit={(event) => {
          event.preventDefault();
          void submitRename();
        }}
      >
        <DialogHeader detail="Keep it short and recognizable" onClose={onClose} title="Rename project" />
        <input
          className="form-control-focus mt-6 h-12 w-full rounded-[0.95rem] border border-border/85 bg-background px-4 text-lg text-ink transition"
          onChange={(event) => setName(event.target.value)}
          ref={inputRef}
          value={name}
        />
        {error ? (
          <div className="mt-3 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
            {apiErrorMessage(error)}
          </div>
        ) : null}
        <div className="mt-6 flex justify-end gap-3">
          <Button className="min-w-24 border-border/80 bg-background text-ink hover:bg-card" onClick={onClose} type="button" variant="secondary">
            Cancel
          </Button>
          <Button className="min-w-24 bg-ink text-background hover:bg-ink/88" disabled={!canSave || saving} type="submit">
            Save
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  );
}


function DialogHeader({
  detail,
  onClose,
  title,
}: {
  detail: string;
  onClose: () => void;
  title: string;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div>
        <h2 className="text-xl font-semibold tracking-normal text-ink">{title}</h2>
        <p className="mt-2 text-sm text-muted">{detail}</p>
      </div>
      <button
        aria-label="Close"
        className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-background hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
        onClick={onClose}
        type="button"
      >
        <X aria-hidden="true" size={18} />
      </button>
    </div>
  );
}
