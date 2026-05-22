import { ChevronRight } from "lucide-react";

import { Badge } from "@/shared/ui/Badge";
import { formatRelativeTime } from "@/shared/lib/time";
import { cn } from "@/shared/lib/cn";
import type { SessionUiStatus } from "@/shared/types/ui";

function statusTone(status: SessionUiStatus) {
  if (status === "running") {
    return "success";
  }
  if (status === "error") {
    return "danger";
  }
  return "default";
}

interface ProjectSessionRowProps {
  label: string;
  lastActivityAt: number;
  model: string;
  onSelect: () => void;
  status: SessionUiStatus;
  title: string;
}

export function ProjectSessionRow({
  label,
  lastActivityAt,
  model,
  onSelect,
  status,
  title,
}: ProjectSessionRowProps) {
  return (
    <button
      aria-label={label}
      className={cn(
        "group flex min-h-[72px] w-full items-center justify-between gap-4 border-b border-border/50 px-4 py-3.5 text-left transition",
        "hover:bg-background/42",
      )}
      onClick={onSelect}
      type="button"
    >
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium text-ink">{title}</div>
        <div className="mt-1 truncate text-xs text-muted">
          {model} · {formatRelativeTime(lastActivityAt)}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-3">
        <Badge className="whitespace-nowrap" tone={statusTone(status)}>
          {status.replaceAll("_", " ")}
        </Badge>
        <ChevronRight className="text-muted/60 transition group-hover:translate-x-0.5 group-hover:text-ink" size={16} />
      </div>
    </button>
  );
}
