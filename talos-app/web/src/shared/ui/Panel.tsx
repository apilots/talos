import type { HTMLAttributes, PropsWithChildren } from "react";

import { cn } from "@/shared/lib/cn";

type PanelProps = HTMLAttributes<HTMLDivElement>;

export function Panel({ children, className, ...props }: PropsWithChildren<PanelProps>) {
  return (
    <div
      className={cn(
        "rounded-[var(--radius-panel)] border border-border bg-card shadow-none",
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
}
