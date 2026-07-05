import { Blocks, FileText, PenLine, Search } from "lucide-react";
import { useTranslation } from "react-i18next";

import { SidebarNavItem } from "./SidebarNavItem";

interface SidebarPrimaryNavProps {
  onCreateThread: () => Promise<void> | void;
}

export function SidebarPrimaryNav({ onCreateThread }: SidebarPrimaryNavProps) {
  const { t } = useTranslation(["chat", "common", "sidebar"]);

  return (
    <nav className="mb-6 space-y-1">
      <button
        className="flex w-full items-center gap-3 rounded-[var(--radius-control)] px-3 py-2.5 text-left text-sm text-ink transition hover:bg-card/88 focus-visible:ring-2 focus-visible:ring-brand/30"
        onClick={() => void onCreateThread()}
        type="button"
      >
        <PenLine aria-hidden="true" size={16} />
        <span>{t("chat:newThread")}</span>
      </button>
      <SidebarNavItem icon={<Search aria-hidden="true" size={16} />} label={t("common:search")} to="/memory" />
      <SidebarNavItem icon={<Blocks aria-hidden="true" size={16} />} label={t("sidebar:plugins")} to="/plugins" />
      <SidebarNavItem icon={<FileText aria-hidden="true" size={16} />} label={t("sidebar:files")} to="/files" />
    </nav>
  );
}
