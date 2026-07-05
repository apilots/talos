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
}

export function SidebarSessionRow({
  currentPath,
  indent = false,
  session,
  onSelectSession,
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
        <span className="whitespace-nowrap text-right text-xs tabular-nums text-muted">{formatRelativeTime(session.lastActivityAt)}</span>
      </button>
    </div>
  );
}
