import type { PropsWithChildren } from "react";

import { cn } from "@/shared/lib/cn";

interface PanelProps {
  className?: string;
}

export function Panel({ children, className }: PropsWithChildren<PanelProps>) {
  return (
    <div className={cn("rounded-[1.5rem] border border-border bg-card shadow-panel", className)}>
      {children}
    </div>
  );
}
