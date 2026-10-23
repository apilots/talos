import * as SelectPrimitive from "@radix-ui/react-select";
import { Check, ChevronDown, ChevronUp } from "lucide-react";

interface SelectProps {
  id?: string;
  value: string;
  options: ReadonlyArray<{ value: string; label: string }>;
  onValueChange: (value: string) => void;
  disabled?: boolean;
  "aria-label"?: string;
  "aria-describedby"?: string;
}

export function Select({ options, onValueChange, value, disabled, ...props }: SelectProps) {
  return (
    <SelectPrimitive.Root disabled={disabled} onValueChange={onValueChange} value={value}>
      <SelectPrimitive.Trigger {...props} className="form-control-focus flex h-10 w-full min-w-0 items-center justify-between gap-2 rounded-[var(--radius-control)] border border-border/85 bg-surface/88 px-3 text-left text-sm text-ink disabled:cursor-not-allowed disabled:opacity-50 [&>span:first-child]:truncate">
        <SelectPrimitive.Value /><SelectPrimitive.Icon asChild><ChevronDown aria-hidden className="shrink-0 text-muted" size={14} /></SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content className="z-[1200] max-h-[var(--radix-select-content-available-height)] min-w-[var(--radix-select-trigger-width)] max-w-[calc(100vw-24px)] overflow-hidden rounded-xl border border-border bg-card p-1 text-ink shadow-panel" collisionPadding={12} position="popper" sideOffset={5}>
          <SelectPrimitive.ScrollUpButton className="flex justify-center py-1 text-muted"><ChevronUp aria-hidden size={14} /></SelectPrimitive.ScrollUpButton>
          <SelectPrimitive.Viewport>
            {options.map((option) => <SelectPrimitive.Item className="relative flex min-h-9 cursor-default select-none items-center rounded-lg py-2 pl-8 pr-3 text-sm outline-none data-[highlighted]:bg-surface" key={option.value} value={option.value}>
              <SelectPrimitive.ItemIndicator className="absolute left-2"><Check aria-hidden size={14} /></SelectPrimitive.ItemIndicator><SelectPrimitive.ItemText>{option.label}</SelectPrimitive.ItemText>
            </SelectPrimitive.Item>)}
          </SelectPrimitive.Viewport>
          <SelectPrimitive.ScrollDownButton className="flex justify-center py-1 text-muted"><ChevronDown aria-hidden size={14} /></SelectPrimitive.ScrollDownButton>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
