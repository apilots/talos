import type { InputHTMLAttributes, TextareaHTMLAttributes } from "react";

import { cn } from "@/shared/lib/cn";

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={cn(
        "w-full rounded-[var(--radius-control)] border border-border/85 bg-surface/88 px-4 py-3 text-sm text-ink outline-none transition placeholder:text-muted focus:border-brand/60 focus:bg-background/92",
        props.className,
      )}
    />
  );
}

export function Textarea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className={cn(
        "w-full rounded-[var(--radius-control)] border border-border/85 bg-surface/88 px-4 py-3 text-sm text-ink outline-none transition placeholder:text-muted focus:border-brand/60 focus:bg-background/92",
        props.className,
      )}
    />
  );
}
