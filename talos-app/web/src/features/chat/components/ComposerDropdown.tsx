import { selectionClasses } from "@/shared/ui/controlStyles";
import { Fragment, useEffect, useRef, useState } from "react";
import type React from "react";
import { Check, ChevronDown } from "lucide-react";

import { cn } from "@/shared/lib/cn";

export interface DropdownItem {
  value: string;
  label: string;
  description?: string;
  disabled?: boolean;
  group?: string;
  icon?: React.ReactNode;
}

interface ComposerDropdownProps {
  header: string;
  items: DropdownItem[];
  value: string | undefined;
  onChange: (value: string) => void;
  triggerIcon?: React.ReactNode;
  className?: string;
  menuClassName?: string;
  disabled?: boolean;
  triggerRef?: React.Ref<HTMLButtonElement>;
}

export function ComposerDropdown({
  header,
  items,
  value,
  onChange,
  triggerIcon,
  className,
  menuClassName,
  disabled = false,
  triggerRef,
}: ComposerDropdownProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selectedItem = items.find((item) => item.value === value);

  useEffect(() => {
    if (!open) {
      return;
    }

    function onPointerDown(event: PointerEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }

    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  return (
    <div className={cn("relative", className)} ref={ref}>
      <button
        className="inline-flex items-center gap-1 rounded-full px-2 py-1 text-[12px] font-medium text-ink/70 outline-none transition hover:bg-hover focus:outline-none"
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
        ref={triggerRef}
        type="button"
      >
        {triggerIcon}
        <span className="max-w-[120px] truncate">{selectedItem?.label ?? value}</span>
        <ChevronDown size={11} className="shrink-0 text-muted" />
      </button>

      {open ? (
        <div
          className={cn(
            "absolute bottom-[calc(100%+6px)] left-0 z-50 min-w-[160px] overflow-hidden rounded-xl border border-border/60 bg-card py-1 shadow-lg",
            menuClassName,
          )}
        >
          <div className="px-3 pb-1 pt-1.5 text-[11px] font-medium text-muted">{header}</div>
          {items.map((item, index) => (
            <Fragment key={item.value}>
              {item.group && item.group !== items[index - 1]?.group ? (
                <div className="px-3 pb-1 pt-2 text-[10px] font-semibold uppercase tracking-wide text-muted">
                  {item.group}
                </div>
              ) : null}
              <button
                aria-disabled={item.disabled}
                className={cn(
                  "flex w-full items-start gap-2 px-3 py-2 text-left text-[13px] text-ink transition focus:outline-none",
                  item.disabled ? "cursor-not-allowed opacity-45" : "hover:bg-hover",
                  item.value === value && selectionClasses.selected,
                )}
                disabled={item.disabled}
                onClick={() => {
                  if (item.disabled) {
                    return;
                  }
                  onChange(item.value);
                  setOpen(false);
                }}
                type="button"
              >
                {item.icon ? (
                  <span className="mt-0.5 shrink-0 text-muted">{item.icon}</span>
                ) : null}
                <span className="min-w-0 flex-1">
                  <span className="block">{item.label}</span>
                  {item.description ? (
                    <span className="mt-0.5 block text-[12px] leading-snug text-muted">
                      {item.description}
                    </span>
                  ) : null}
                </span>
                {item.value === value ? (
                  <Check size={12} className="shrink-0 text-brand" />
                ) : null}
              </button>
            </Fragment>
          ))}
        </div>
      ) : null}
    </div>
  );
}
