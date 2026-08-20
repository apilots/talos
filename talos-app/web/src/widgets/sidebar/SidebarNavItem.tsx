import type { ReactNode } from "react";
import { NavLink } from "react-router-dom";

import { cn } from "@/shared/lib/cn";
import { sidebarInteractiveRowClass } from "./sidebarRowStyles";

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
          sidebarInteractiveRowClass,
          "flex items-center gap-3 px-3 py-2.5 text-sm text-muted hover:text-ink",
          isActive && "text-ink",
        )
      }
      to={to}
    >
      {icon}
      <span>{label}</span>
    </NavLink>
  );
}
