import { Pin, PinOff } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";

interface SidebarSessionPinButtonProps {
  sessionId: string;
  pinned: boolean;
  sessionLabel: string;
  pending?: boolean;
  onToggle: () => void;
}

export function SidebarSessionPinButton({
  sessionId,
  pinned,
  sessionLabel,
  pending = false,
  onToggle,
}: SidebarSessionPinButtonProps) {
  const { t } = useTranslation("sidebar");
  const label = t(pinned ? "unpinSession" : "pinSession", { title: sessionLabel });

  return (
    <button
      aria-disabled={pending || undefined}
      aria-label={label}
      aria-pressed={pinned}
      className={cn(
        "flex size-6 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-hover hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30",
        pending && "pointer-events-none opacity-50",
        pinned && "text-ink",
      )}
      data-sidebar-pin-button={sessionId}
      onClick={(event) => {
        event.stopPropagation();
        if (pending) {
          return;
        }
        onToggle();
      }}
      title={label}
      type="button"
    >
      {pinned ? (
        <PinOff aria-hidden="true" size={14} />
      ) : (
        <Pin aria-hidden="true" size={14} />
      )}
    </button>
  );
}
