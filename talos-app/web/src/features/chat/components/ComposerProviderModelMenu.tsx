import { Check, ChevronRight } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import type { DropdownItem } from "./ComposerDropdown";

export function ComposerProviderModelMenuRow({
  active = false,
  disabled = false,
  label,
  onClick,
  onHover,
  value,
}: {
  active?: boolean;
  disabled?: boolean;
  label: string;
  onClick: () => void;
  onHover: () => void;
  value: string;
}) {
  return (
    <button
      aria-expanded={active}
      aria-label={`${label}: ${value}`}
      className={cn(
        "grid w-full grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)_16px] items-center gap-2 rounded-xl px-3 py-2.5 text-left text-[13px] outline-none transition-[background-color,transform] duration-150 active:scale-[0.99] active:bg-ink/[0.045] focus-visible:ring-2 focus-visible:ring-brand/20 disabled:cursor-default disabled:opacity-45 disabled:active:scale-100 max-sm:grid-cols-[minmax(0,1fr)_16px] max-sm:gap-y-1",
        active && "bg-ink/[0.025]",
      )}
      disabled={disabled}
      onClick={onClick}
      onMouseEnter={disabled ? undefined : onHover}
      type="button"
    >
      <span className="font-medium text-ink">{label}</span>
      <span className="min-w-0 text-right text-muted [overflow-wrap:anywhere] max-sm:col-start-1 max-sm:row-start-2 max-sm:text-left">{value}</span>
      <ChevronRight className="text-muted/70 max-sm:col-start-2 max-sm:row-span-2 max-sm:row-start-1" size={15} />
    </button>
  );
}

export function ComposerProviderModelSelectionPage({ items, onChange, value }: {
  items: DropdownItem[];
  onChange: (value: string) => void;
  value: string | undefined;
}) {
  return (
    <div className="absolute right-0 top-0 w-[calc(50%-4px)] min-w-0 rounded-[1.1rem] border border-border/55 bg-card p-1.5 shadow-[0_12px_32px_rgba(0,0,0,0.075)]">
      <div className="max-h-64 overflow-y-auto py-0.5">
        {items.map((item) => (
          <button
            className={cn(
              "flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-[13px] text-ink outline-none transition-[background-color,transform] duration-150 hover:bg-ink/[0.025] active:scale-[0.99] active:bg-ink/[0.045] focus-visible:bg-ink/[0.025] disabled:cursor-not-allowed disabled:opacity-45",
              item.value === value && "bg-ink/[0.02]",
            )}
            disabled={item.disabled}
            key={item.value}
            onClick={() => onChange(item.value)}
            type="button"
          >
            <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">{item.label}</span>
            {item.value === value ? <Check className="shrink-0 text-brand" size={13} /> : null}
          </button>
        ))}
      </div>
    </div>
  );
}
