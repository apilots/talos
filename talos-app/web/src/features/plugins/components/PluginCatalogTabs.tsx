import { selectionClasses } from "@/shared/ui/controlStyles";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import type { PluginsCatalogTab } from "../types";

interface PluginCatalogTabsProps {
  activeTab: PluginsCatalogTab;
  onTabChange: (tab: PluginsCatalogTab) => void;
}

export function PluginCatalogTabs({ activeTab, onTabChange }: PluginCatalogTabsProps) {
  const { t } = useTranslation("plugins");

  return (
    <div className="mb-5 flex items-center gap-1">
      <TabButton active={activeTab === "plugins"} onClick={() => onTabChange("plugins")}>
        {t("tabs.plugins")}
      </TabButton>
      <TabButton active={activeTab === "skills"} onClick={() => onTabChange("skills")}>
        {t("tabs.skills")}
      </TabButton>
      <TabButton active={activeTab === "mcp"} onClick={() => onTabChange("mcp")}>
        {t("tabs.mcp")}
      </TabButton>
    </div>
  );
}

function TabButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: string;
  onClick: () => void;
}) {
  return (
    <button
      className={cn(
        "inline-flex h-9 items-center rounded-[0.55rem] border px-3 text-sm transition-colors",
        active
          ? `${selectionClasses.selected} ${selectionClasses.border}`
          : `border-transparent ${selectionClasses.idle}`,
      )}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  );
}
