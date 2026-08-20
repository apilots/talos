import { useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

import { Button } from "@/shared/ui/Button";

interface SidebarProjectRemoveDialogProps {
  projectName: string;
  removing: boolean;
  onClose: () => void;
  onRemove: () => Promise<void> | void;
}

export function SidebarProjectRemoveDialog({
  projectName,
  removing,
  onClose,
  onRemove,
}: SidebarProjectRemoveDialogProps) {
  const [error, setError] = useState<string | null>(null);

  async function submitRemove() {
    if (removing) {
      return;
    }
    setError(null);
    try {
      await onRemove();
    } catch (submitError) {
      setError(errorMessage(submitError));
    }
  }

  return createPortal(
    <div className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/30 px-4 backdrop-blur-[2px]">
      <div className="absolute inset-0" onClick={onClose} role="presentation" />
      <div className="relative z-10 w-[min(560px,calc(100vw-32px))] rounded-[var(--radius-panel)] border border-border bg-card p-6 shadow-panel">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-2xl font-semibold tracking-normal text-ink">Remove {projectName}?</h2>
            <p className="mt-3 max-w-[29rem] text-lg leading-8 text-muted">
              This removes the project from Talos. Files on disk will not be deleted.
            </p>
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
        {error ? (
          <div className="mt-4 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
            {error}
          </div>
        ) : null}
        <div className="mt-7 flex justify-end gap-4">
          <Button className="min-w-24" onClick={onClose} type="button" variant="ghost">
            Cancel
          </Button>
          <Button className="min-w-28 bg-danger/12 text-danger hover:bg-danger/20" disabled={removing} onClick={() => void submitRemove()} type="button">
            Remove
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function errorMessage(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
