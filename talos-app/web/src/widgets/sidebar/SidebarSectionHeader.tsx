import { ChevronRight } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { sidebarInteractiveRowClass } from "./sidebarRowStyles";

interface SidebarSectionHeaderProps {
  label: string;
  open: boolean;
  onToggle: () => void;
}

export function SidebarSectionHeader({ label, open, onToggle }: SidebarSectionHeaderProps) {
  return (
    <button
      className={cn(
        sidebarInteractiveRowClass,
        "flex h-8 items-center justify-between px-2 text-left text-sm font-medium text-muted hover:text-ink",
      )}
      onClick={onToggle}
      type="button"
    >
      <span>{label}</span>
      <ChevronRight className={cn("transition-transform", open && "rotate-90")} size={15} />
    </button>
  );
}
