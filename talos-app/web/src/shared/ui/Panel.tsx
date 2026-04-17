import type { PropsWithChildren } from "react";

import { cn } from "@/shared/lib/cn";

interface PanelProps {
  className?: string;
}

export function Panel({ children, className }: PropsWithChildren<PanelProps>) {
  return (
    <div
      className={cn(
        "rounded-[var(--radius-panel)] border border-border/80 bg-card/90 shadow-panel backdrop-blur-[14px]",
        className,
      )}
    >
      {children}
    </div>
  );
}
