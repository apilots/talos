import { PanelLeftClose } from "lucide-react";

interface SidebarCollapseButtonProps {
  onCollapse: () => void;
}

export function SidebarCollapseButton({ onCollapse }: SidebarCollapseButtonProps) {
  return (
    <button
      aria-label="Collapse sidebar"
      className="flex size-7 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-card/75 hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/30"
      onClick={onCollapse}
      type="button"
    >
      <PanelLeftClose size={15} />
    </button>
  );
}
