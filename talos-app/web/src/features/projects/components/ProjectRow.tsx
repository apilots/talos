import { ArrowUpRight, FolderRoot } from "lucide-react";

import type { ProjectRecord } from "@/shared/types/ui";
import { cn } from "@/shared/lib/cn";
import { formatRelativeTime } from "@/shared/lib/time";

interface ProjectRowProps {
  project: ProjectRecord;
  label: string;
  onSelect: () => void;
}

export function ProjectRow({ project, label, onSelect }: ProjectRowProps) {
  return (
    <button
      aria-label={label}
      className={cn(
        "group relative min-h-[164px] overflow-hidden rounded-[var(--radius-panel)] border border-border/75 bg-card/92 p-6 text-left shadow-panel transition",
        "hover:-translate-y-0.5 hover:border-brand/30 hover:bg-card/98",
      )}
      onClick={onSelect}
      type="button"
    >
      <div className="flex items-start justify-between gap-3">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-[var(--radius-control)] border border-border/65 bg-background/42 text-brand/80 transition group-hover:border-brand/30 group-hover:bg-brand/10">
          <FolderRoot size={16} />
        </span>
        <span className="flex size-8 shrink-0 items-center justify-center rounded-full border border-border/65 bg-background/38 text-muted/65 transition group-hover:border-brand/25 group-hover:text-brand">
          <ArrowUpRight size={14} />
        </span>
      </div>
      <div className="mt-10 min-w-0">
        <div className="truncate text-[18px] font-semibold tracking-[-0.035em] text-ink">{project.name}</div>
        <div className="mt-4 text-sm text-muted">Updated {formatRelativeTime(project.updatedAt)}</div>
      </div>
    </button>
  );
}
