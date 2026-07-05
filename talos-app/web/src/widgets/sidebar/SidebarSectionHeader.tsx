import { ChevronRight } from "lucide-react";

import { cn } from "@/shared/lib/cn";

interface SidebarSectionHeaderProps {
  label: string;
  open: boolean;
  onToggle: () => void;
}

export function SidebarSectionHeader({ label, open, onToggle }: SidebarSectionHeaderProps) {
  return (
    <button
      className="flex h-8 w-full items-center justify-between rounded-[var(--radius-control)] px-2 text-left text-sm font-medium text-muted transition hover:bg-card/55 hover:text-ink"
      onClick={onToggle}
      type="button"
    >
      <span>{label}</span>
      <ChevronRight className={cn("transition-transform", open && "rotate-90")} size={15} />
    </button>
  );
}
