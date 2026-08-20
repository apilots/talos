import { Blocks, Brain, FileText, PenLine } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import { SidebarNavItem } from "./SidebarNavItem";
import { sidebarInteractiveRowClass } from "./sidebarRowStyles";

interface SidebarPrimaryNavProps {
  onCreateThread: () => Promise<void> | void;
}

export function SidebarPrimaryNav({ onCreateThread }: SidebarPrimaryNavProps) {
  const { t } = useTranslation(["chat", "common", "sidebar"]);

  return (
    <nav className="mb-6 space-y-1">
      <button
        className={cn(
          sidebarInteractiveRowClass,
          "flex items-center gap-3 px-3 py-2.5 text-left text-sm text-ink",
        )}
        onClick={() => void onCreateThread()}
        type="button"
      >
        <PenLine aria-hidden="true" size={16} />
        <span>{t("chat:newThread")}</span>
      </button>
      <SidebarNavItem icon={<Brain aria-hidden="true" size={16} />} label={t("sidebar:memory")} to="/memory" />
      <SidebarNavItem icon={<Blocks aria-hidden="true" size={16} />} label={t("sidebar:plugins")} to="/plugins" />
      <SidebarNavItem icon={<FileText aria-hidden="true" size={16} />} label={t("sidebar:files")} to="/files" />
    </nav>
  );
}
