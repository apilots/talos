import type { ReactNode } from "react";
import { NavLink } from "react-router-dom";

import { cn } from "@/shared/lib/cn";

interface SidebarNavItemProps {
  icon: ReactNode;
  label: string;
  to: string;
}

export function SidebarNavItem({ icon, label, to }: SidebarNavItemProps) {
  return (
    <NavLink
      className={({ isActive }) =>
        cn(
          "flex items-center gap-3 rounded-[var(--radius-control)] px-3 py-2.5 text-sm text-muted transition hover:bg-card/88 hover:text-ink",
          isActive && "bg-card/92 text-ink shadow-[inset_0_0_0_1px_rgba(224,218,209,0.5)]",
        )
      }
      to={to}
    >
      {icon}
      <span>{label}</span>
    </NavLink>
  );
}
