import { Trash2 } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { formatRelativeTime } from "@/shared/lib/time";
import type { SessionUiStatus } from "@/shared/types/ui";

export interface SidebarSessionItem {
  id: string;
  label: string;
  status: SessionUiStatus;
  model: string;
  projectId?: string | null;
  lastActivityAt: number;
  createdAt: number;
}

interface SidebarSessionRowProps {
  currentPath: string;
  indent?: boolean;
  session: SidebarSessionItem;
  onSelectSession: (sessionId: string) => void;
  onDeleteSession: (session: SidebarSessionItem) => void;
}

export function SidebarSessionRow({
  currentPath,
  indent = false,
  session,
  onSelectSession,
  onDeleteSession,
}: SidebarSessionRowProps) {
  const active = currentPath === `/sessions/${session.id}`;

  return (
    <div className={cn("group relative", indent && "ml-7")} key={`${session.id}-${session.createdAt}`}>
      <button
        className={cn(
          "grid h-8 w-full grid-cols-[minmax(0,1fr)_4.25rem] items-center gap-2 rounded-[var(--radius-control)] px-3 text-left text-sm transition hover:bg-card/70 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30",
          active && "bg-ink/[0.07] text-ink hover:bg-ink/[0.07]",
        )}
        onClick={() => onSelectSession(session.id)}
        type="button"
      >
        <span className="truncate font-medium leading-5 text-ink">{session.label}</span>
        <span
          className={cn(
            "whitespace-nowrap text-right text-xs tabular-nums text-muted transition-opacity",
            active ? "opacity-0" : "group-hover:opacity-0 group-focus-within:opacity-0",
          )}
        >
          {formatRelativeTime(session.lastActivityAt)}
        </span>
      </button>
      <button
        aria-label={`Delete ${session.label}`}
        className={cn(
          "absolute right-2 top-1/2 size-6 -translate-y-1/2 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-danger/10 hover:text-danger focus:outline-none focus-visible:ring-2 focus-visible:ring-danger/30",
          active ? "flex" : "hidden group-hover:flex group-focus-within:flex",
        )}
        onClick={(event) => {
          event.stopPropagation();
          onDeleteSession(session);
        }}
        title={`Delete ${session.label}`}
        type="button"
      >
        <Trash2 aria-hidden="true" size={14} />
      </button>
    </div>
  );
}
