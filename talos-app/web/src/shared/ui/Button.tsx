import type { ButtonHTMLAttributes, PropsWithChildren } from "react";

import { cn } from "@/shared/lib/cn";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
}

const variantClasses: Record<ButtonVariant, string> = {
  primary:
    "bg-brand text-background hover:bg-brand/90 disabled:bg-brandDim disabled:text-muted",
  secondary:
    "border border-border bg-card/80 text-ink hover:border-brand/40 hover:bg-surface disabled:text-muted",
  ghost: "text-muted hover:bg-surface/80 hover:text-ink",
  danger: "bg-danger/15 text-danger hover:bg-danger/25",
};

export function Button({
  children,
  className,
  variant = "primary",
  ...props
}: PropsWithChildren<ButtonProps>) {
  return (
    <button
      className={cn(
        "inline-flex items-center justify-center gap-2 rounded-[var(--radius-control)] px-3.5 py-2.5 text-sm font-medium transition disabled:cursor-not-allowed",
        variantClasses[variant],
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}
