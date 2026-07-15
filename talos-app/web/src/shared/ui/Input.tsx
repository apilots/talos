import { forwardRef } from "react";
import type { InputHTMLAttributes, TextareaHTMLAttributes } from "react";

import { cn } from "@/shared/lib/cn";

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={cn(
        "form-control-focus w-full rounded-[var(--radius-control)] border border-border/85 bg-surface/88 px-4 py-3 text-sm text-ink transition placeholder:text-muted",
        props.className,
      )}
    />
  );
}

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(
  function Textarea(props, ref) {
    return (
      <textarea
        {...props}
        className={cn(
          "form-control-focus w-full rounded-[var(--radius-control)] border border-border/85 bg-surface/88 px-4 py-3 text-sm text-ink transition placeholder:text-muted",
          props.className,
        )}
        ref={ref}
      />
    );
  },
);
