import * as MenuPrimitive from "@radix-ui/react-dropdown-menu";
import { forwardRef, type ComponentPropsWithoutRef, type ElementRef } from "react";
import { cn } from "@/shared/lib/cn";

export const DropdownMenu = MenuPrimitive.Root;
export const DropdownMenuTrigger = MenuPrimitive.Trigger;
export const DropdownMenuSeparator = MenuPrimitive.Separator;

export const DropdownMenuContent = forwardRef<ElementRef<typeof MenuPrimitive.Content>, ComponentPropsWithoutRef<typeof MenuPrimitive.Content>>(function DropdownMenuContent({ className, sideOffset = 6, ...props }, ref) {
  return <MenuPrimitive.Portal><MenuPrimitive.Content {...props} className={cn("z-[1200] min-w-40 max-w-[calc(100vw-24px)] rounded-xl border border-border bg-card p-1 text-ink shadow-panel", className)} collisionPadding={12} ref={ref} sideOffset={sideOffset} /></MenuPrimitive.Portal>;
});

export const DropdownMenuItem = forwardRef<ElementRef<typeof MenuPrimitive.Item>, ComponentPropsWithoutRef<typeof MenuPrimitive.Item>>(function DropdownMenuItem({ className, ...props }, ref) {
  return <MenuPrimitive.Item {...props} className={cn("flex min-h-9 cursor-default select-none items-center gap-2 rounded-lg px-3 py-2 text-sm outline-none data-[highlighted]:bg-surface data-[disabled]:opacity-40", className)} ref={ref} />;
});
