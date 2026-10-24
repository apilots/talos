import { Pencil, X } from "lucide-react";
import type { ReactNode } from "react";

interface SidebarProjectActionsMenuProps {
  onRemove: () => void;
  onRename: () => void;
}

export function SidebarProjectActionsMenu({
  onRemove,
  onRename,
}: SidebarProjectActionsMenuProps) {
  return (
    <div
      className="absolute right-2 top-8 z-40 w-[12.5rem] overflow-hidden rounded-[var(--radius-panel)] border border-border bg-card p-2 shadow-panel"
      data-sidebar-project-menu-root=""
    >
      <ProjectMenuButton icon={<Pencil aria-hidden="true" size={16} />} label="Rename project" onClick={onRename} />
      <ProjectMenuButton icon={<X aria-hidden="true" size={16} />} label="Remove" onClick={onRemove} />
    </div>
  );
}

function ProjectMenuButton({
  icon,
  label,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  onClick?: () => void;
}) {
  return (
    <button
      className="flex h-9 w-full items-center gap-3 rounded-[var(--radius-control)] px-2 text-left text-sm text-ink transition hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
      onClick={onClick}
      type="button"
    >
      <span className="text-muted">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </button>
  );
}
