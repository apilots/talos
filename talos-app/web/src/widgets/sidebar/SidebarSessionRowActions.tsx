import { Trash2 } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { SidebarSessionPinButton } from "./SidebarSessionPinButton";

interface SidebarSessionRowActionsProps {
  sessionId: string;
  active: boolean;
  running: boolean;
  pinned: boolean;
  pinPending?: boolean;
  sessionLabel: string;
  onTogglePin?: () => void;
  onDelete: () => void;
}

export function SidebarSessionRowActions({
  sessionId,
  active,
  running,
  pinned,
  pinPending = false,
  sessionLabel,
  onTogglePin,
  onDelete,
}: SidebarSessionRowActionsProps) {
  return (
    <div
      className={cn(
        "absolute right-2 top-1/2 -translate-y-1/2 flex items-center gap-0.5 transition-opacity",
        active && !running
          ? "opacity-100 pointer-events-auto"
          : "opacity-0 pointer-events-none group-hover:opacity-100 group-hover:pointer-events-auto group-focus-within:opacity-100 group-focus-within:pointer-events-auto",
      )}
    >
      {onTogglePin ? (
        <SidebarSessionPinButton
          onToggle={onTogglePin}
          pending={pinPending}
          pinned={pinned}
          sessionId={sessionId}
          sessionLabel={sessionLabel}
        />
      ) : null}
      <button
        aria-label={`Delete ${sessionLabel}`}
        className="flex size-6 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-danger/10 hover:text-danger focus:outline-none focus-visible:ring-2 focus-visible:ring-danger/30"
        onClick={(event) => {
          event.stopPropagation();
          onDelete();
        }}
        title={`Delete ${sessionLabel}`}
        type="button"
      >
        <Trash2 aria-hidden="true" size={14} />
      </button>
    </div>
  );
}
