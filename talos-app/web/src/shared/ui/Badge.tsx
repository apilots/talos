import type { PropsWithChildren } from "react";

import { cn } from "@/shared/lib/cn";

interface BadgeProps {
  tone?: "default" | "success" | "warning" | "danger";
  className?: string;
}

const toneClasses = {
  default: "border-border bg-background/60 text-muted",
  success: "border-success/25 bg-success/10 text-success",
  warning: "border-warning/25 bg-warning/10 text-warning",
  danger: "border-danger/25 bg-danger/10 text-danger",
};

export function Badge({ children, tone = "default", className }: PropsWithChildren<BadgeProps>) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full border px-2.5 py-1 text-[11px] uppercase tracking-[0.16em]",
        toneClasses[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
