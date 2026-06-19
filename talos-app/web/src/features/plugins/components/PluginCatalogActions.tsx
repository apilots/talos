import { Box, ChevronDown, CircleDot, Layers3, Plus, RefreshCw } from "lucide-react";

import type { PluginsCatalogTab } from "../types";

interface PluginCatalogActionsProps {
  activeTab: PluginsCatalogTab;
  menuOpen: boolean;
  onAddMarketplace: () => void;
  onCreatePlugin: () => void;
  onCreateSkill: () => Promise<void> | void;
  onMenuOpenChange: (open: boolean) => void;
  onRefresh: () => Promise<unknown> | void;
}

export function PluginCatalogActions({
  activeTab,
  menuOpen,
  onAddMarketplace,
  onCreatePlugin,
  onCreateSkill,
  onMenuOpenChange,
  onRefresh,
}: PluginCatalogActionsProps) {
  return (
    <div className="relative flex items-center gap-2">
      <button
        className="inline-flex h-9 items-center gap-1 rounded-full border border-border/80 bg-card px-3 text-sm text-ink shadow-sm transition hover:border-brand/35"
        onClick={() => onMenuOpenChange(!menuOpen)}
        type="button"
      >
        <Plus size={16} />
        <ChevronDown size={14} />
      </button>
      <button
        className="inline-flex h-9 w-9 items-center justify-center rounded-full border border-border/80 bg-card text-muted shadow-sm transition hover:border-brand/35 hover:text-ink"
        onClick={() => void onRefresh()}
        type="button"
      >
        <RefreshCw size={15} />
      </button>
      {menuOpen ? (
        <div className="absolute right-0 top-11 z-20 w-56 rounded-[1rem] border border-border/80 bg-card p-1.5 shadow-[0_18px_36px_rgba(35,32,28,0.14)]">
          {activeTab === "skills" ? (
            <SkillsMenu onCreateSkill={onCreateSkill} />
          ) : (
            <PluginsMenu onAddMarketplace={onAddMarketplace} onCreatePlugin={onCreatePlugin} />
          )}
        </div>
      ) : null}
    </div>
  );
}

function SkillsMenu({ onCreateSkill }: { onCreateSkill: () => Promise<void> | void }) {
  return (
    <>
      <button
        className="flex w-full items-center gap-2 rounded-[0.8rem] px-3 py-2 text-left text-sm text-ink transition hover:bg-background/80"
        onClick={() => void onCreateSkill()}
        type="button"
      >
        <Box size={15} />
        Create skill
      </button>
      <button
        className="flex w-full items-center gap-2 rounded-[0.8rem] px-3 py-2 text-left text-sm text-muted"
        disabled
        type="button"
      >
        <CircleDot size={15} />
        Record a skill
      </button>
    </>
  );
}

function PluginsMenu({
  onAddMarketplace,
  onCreatePlugin,
}: {
  onAddMarketplace: () => void;
  onCreatePlugin: () => void;
}) {
  return (
    <>
      <button
        className="flex w-full items-center gap-2 rounded-[0.8rem] px-3 py-2 text-left text-sm text-ink transition hover:bg-background/80"
        onClick={onCreatePlugin}
        type="button"
      >
        <Layers3 size={15} />
        Create plugin
      </button>
      <button
        className="flex w-full items-center gap-2 rounded-[0.8rem] px-3 py-2 text-left text-sm text-ink transition hover:bg-background/80"
        onClick={onAddMarketplace}
        type="button"
      >
        <Box size={15} />
        Add marketplace
      </button>
    </>
  );
}
