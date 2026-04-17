import { cn } from "@/shared/lib/cn";
import { SkeletonLine } from "@/shared/ui/SkeletonLine";

interface ChatContextBarProps {
  loading?: boolean;
  projectName?: string;
  sessionTitle?: string;
  onProjectClick?: () => void;
}

export function ChatContextBar({
  loading = false,
  projectName,
  sessionTitle,
  onProjectClick,
}: ChatContextBarProps) {
  return (
    <div className="shrink-0 border-b border-border/55 bg-background/72 backdrop-blur-[14px]">
      <div className="flex h-12 w-full items-center px-7">
        {loading ? (
          <SkeletonLine className="h-5 w-[280px]" />
        ) : (
          <div className="flex min-w-0 items-center gap-2 text-[15px] tracking-[-0.02em]">
            {projectName ? (
              <>
                <button
                  className={cn(
                    "max-w-[260px] truncate text-sm font-medium text-muted transition hover:text-ink",
                    !onProjectClick && "cursor-default hover:text-muted",
                  )}
                  disabled={!onProjectClick}
                  onClick={() => onProjectClick?.()}
                  type="button"
                >
                  {projectName}
                </button>
                <span className="text-sm text-muted/45">/</span>
              </>
            ) : null}
            <div className="truncate text-sm font-medium text-ink">{sessionTitle ?? ""}</div>
          </div>
        )}
      </div>
    </div>
  );
}
