import { PanelLeft } from "lucide-react";

interface SidebarCollapseButtonProps {
  onCollapse: () => void;
}

export function SidebarCollapseButton({ onCollapse }: SidebarCollapseButtonProps) {
  return (
    <button
      aria-label="Collapse sidebar"
      className="flex size-7 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-selected/60 hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
      onClick={onCollapse}
      type="button"
    >
      <PanelLeft aria-hidden="true" size={15} />
    </button>
  );
}
