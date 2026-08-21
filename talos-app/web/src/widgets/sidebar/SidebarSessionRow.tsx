import { LoaderCircle, Trash2 } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { formatRelativeTime } from "@/shared/lib/time";
import type { SessionUiStatus } from "@/shared/types/ui";
import { sidebarInteractiveRowClass } from "./sidebarRowStyles";

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
  const running = session.status === "running";

  return (
    <div className="group relative" key={`${session.id}-${session.createdAt}`}>
      <button
        className={cn(
          sidebarInteractiveRowClass,
          "grid h-8 grid-cols-[minmax(0,1fr)_4.25rem] items-center gap-2 text-left text-sm",
          indent ? "pl-10 pr-3" : "px-3",
          active && "text-ink",
        )}
        data-selected={active ? "true" : undefined}
        onClick={() => onSelectSession(session.id)}
        type="button"
      >
        <span className="truncate font-medium leading-5 text-ink">{session.label}</span>
        <span
          aria-label={running ? `Running ${session.label}` : undefined}
          className={cn(
            "flex justify-end text-muted transition-opacity",
            !running && "whitespace-nowrap text-xs tabular-nums",
            active && !running
              ? "opacity-0"
              : "group-hover:opacity-0 group-focus-within:opacity-0",
          )}
          role={running ? "status" : undefined}
        >
          {running ? (
            <LoaderCircle
              aria-hidden="true"
              className="motion-safe:animate-spin motion-safe:[animation-duration:1.6s]"
              size={14}
            />
          ) : (
            formatRelativeTime(session.lastActivityAt)
          )}
        </span>
      </button>
      <button
        aria-label={`Delete ${session.label}`}
        className={cn(
          "absolute right-2 top-1/2 size-6 -translate-y-1/2 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-danger/10 hover:text-danger focus:outline-none focus-visible:ring-2 focus-visible:ring-danger/30",
          active && !running ? "flex" : "hidden group-hover:flex group-focus-within:flex",
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
