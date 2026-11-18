import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";

export function TimelineRailRow({
  children,
  icon: Icon,
  spinning = false,
}: {
  children: ReactNode;
  icon: LucideIcon;
  spinning?: boolean;
}) {
  return (
    <div className="relative grid min-w-0 grid-cols-[1.5rem_minmax(0,1fr)] items-start gap-2 before:pointer-events-none before:absolute before:-bottom-3 before:left-[0.72rem] before:top-5 before:w-px before:bg-border/65 last:before:hidden">
      <span aria-hidden="true" className="relative mt-0.5 flex size-6 items-center justify-center rounded-full bg-background text-muted" data-timeline-marker="true">
        <Icon className={cn(spinning && "motion-safe:animate-spin")} size={13} />
      </span>
      <div className="min-w-0">{children}</div>
    </div>
  );
}
