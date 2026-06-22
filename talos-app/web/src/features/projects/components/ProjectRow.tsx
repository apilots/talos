import { ArrowUpRight, FolderRoot, Trash2, TriangleAlert } from "lucide-react";

import type { ProjectView } from "@/shared/types/ui";
import { cn } from "@/shared/lib/cn";
import { formatRelativeTime } from "@/shared/lib/time";

interface ProjectRowProps {
  project: ProjectView;
  label: string;
  onSelect: () => void;
  onDelete: () => void;
  deleteLabel: string;
  deleteDisabled?: boolean;
  deleteDisabledReason?: string;
  deleting?: boolean;
}

export function ProjectRow({
  project,
  label,
  onDelete,
  onSelect,
  deleteLabel,
  deleteDisabled = false,
  deleteDisabledReason,
  deleting = false,
}: ProjectRowProps) {
  const unavailable = Boolean(project.status && project.status.state !== "available");

  return (
    <div
      className={cn(
        "group relative min-h-[164px] overflow-hidden rounded-[var(--radius-panel)] border border-border/75 bg-card/92 shadow-panel transition",
        "hover:-translate-y-0.5 hover:border-brand/30 hover:bg-card/98",
      )}
    >
      <button
        aria-label={label}
        className="h-full w-full p-6 text-left"
        onClick={onSelect}
        type="button"
      >
        <div className="flex items-start justify-between gap-3">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-[var(--radius-control)] border border-border/65 bg-background/42 text-brand/80 transition group-hover:border-brand/30 group-hover:bg-brand/10">
            <FolderRoot size={16} />
          </span>
          <span
            className={cn(
              "flex size-8 shrink-0 items-center justify-center rounded-full border border-border/65 bg-background/38 transition group-hover:border-brand/25",
              unavailable ? "text-danger" : "text-muted/65 group-hover:text-brand",
            )}
            title={unavailable ? "Project directory is unavailable." : undefined}
          >
            {unavailable ? <TriangleAlert size={14} /> : <ArrowUpRight size={14} />}
          </span>
        </div>
        <div className="mt-10 min-w-0">
          <div className="truncate pr-10 text-[18px] font-semibold tracking-[-0.035em] text-ink">{project.name}</div>
          <div className="mt-4 text-sm text-muted">Updated {formatRelativeTime(project.updatedAt)}</div>
        </div>
      </button>

      <button
        aria-label={deleteLabel}
        className={cn(
          "absolute bottom-4 right-4 inline-flex h-9 items-center gap-1.5 rounded-[999px] px-3 text-sm transition",
          deleteDisabled
            ? "cursor-not-allowed border border-border/60 bg-background/78 text-muted/70 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
            : "border border-danger/20 bg-card/92 text-danger opacity-0 shadow-[0_8px_24px_rgba(95,84,64,0.08)] hover:bg-danger/8 group-hover:opacity-100 group-focus-within:opacity-100",
        )}
        disabled={deleteDisabled || deleting}
        onClick={(event) => {
          event.stopPropagation();
          onDelete();
        }}
        title={deleteDisabled ? deleteDisabledReason : deleteLabel}
        type="button"
      >
        <Trash2 size={14} />
        {deleteLabel}
      </button>
    </div>
  );
}
