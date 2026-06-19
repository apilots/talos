import { cn } from "@/shared/lib/cn";
import type { PluginsCatalogTab } from "../types";

interface PluginCatalogTabsProps {
  activeTab: PluginsCatalogTab;
  onTabChange: (tab: PluginsCatalogTab) => void;
}

export function PluginCatalogTabs({ activeTab, onTabChange }: PluginCatalogTabsProps) {
  return (
    <div className="mb-5 flex items-center gap-1">
      <TabButton active={activeTab === "plugins"} onClick={() => onTabChange("plugins")}>
        Plugins
      </TabButton>
      <TabButton active={activeTab === "skills"} onClick={() => onTabChange("skills")}>
        Skills
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
        "rounded-[0.55rem] px-3 py-2 text-sm transition",
        active ? "bg-card text-ink shadow-sm" : "text-muted hover:bg-card/65 hover:text-ink",
      )}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  );
}
