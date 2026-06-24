import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";
import type { SkillSuggestion, SkillSummary } from "@talos/sdk-browser";

import {
  addMarketplaceSource,
  AddPluginMarketplaceDialog,
  AllPluginsManagementView,
  listSkills,
  McpPluginsManagementView,
  PluginCatalogActions,
  PluginCatalogSearch,
  PluginCatalogTabs,
  SkillPluginsManagementView,
  type PluginsCatalogTab,
} from "@/features/plugins";

export function PluginsPage() {
  const { t } = useTranslation("plugins");
  const navigate = useNavigate();
  const location = useLocation();
  const locationState = location.state as { initialTab?: PluginsCatalogTab } | null;
  const [tab, setTab] = useState<PluginsCatalogTab>(locationState?.initialTab ?? "plugins");
  const [query, setQuery] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [marketplaceOpen, setMarketplaceOpen] = useState(false);
  const [refreshVersion, setRefreshVersion] = useState(0);

  async function createSkill() {
    setMenuOpen(false);
    const skills = await listSkills(true);
    const creator = findSkillCreator(skills.data);
    navigate("/", {
      state: {
        initialDraft: creator ? "" : t("drafts.createSkill"),
        initialSkills: creator ? [toSuggestion(creator)] : [],
      },
    });
  }

  function createPlugin() {
    setMenuOpen(false);
    navigate("/", { state: { initialDraft: t("drafts.createPlugin") } });
  }

  function addMarketplace() {
    setMenuOpen(false);
    setMarketplaceOpen(true);
  }

  function refresh() {
    setRefreshVersion((version) => version + 1);
  }

  async function submitMarketplace(input: { source: string; gitRef: string; sparsePaths: string }) {
    await addMarketplaceSource(input);
    refresh();
  }

  return (
    <div className="h-full overflow-y-auto bg-background px-8 py-7 text-ink">
      <div className="mx-auto max-w-[920px]">
        <div className="flex items-start justify-between gap-4">
          <div>
            <PluginCatalogTabs activeTab={tab} onTabChange={setTab} />
            <h1 className="font-display text-[34px] font-semibold tracking-tight">
              {t(`tabs.${tab}`)}
            </h1>
            <p className="mt-1 text-[15px] text-muted">{t(`page.${tab}.detail`)}</p>
          </div>
          <PluginCatalogActions
            activeTab={tab}
            menuOpen={menuOpen}
            onAddMarketplace={addMarketplace}
            onCreatePlugin={createPlugin}
            onCreateSkill={createSkill}
            onMenuOpenChange={setMenuOpen}
            onRefresh={refresh}
          />
        </div>
        <PluginCatalogSearch onQueryChange={setQuery} query={query} />
        {tab === "plugins" ? (
          <AllPluginsManagementView query={query} refreshVersion={refreshVersion} />
        ) : tab === "skills" ? (
          <SkillPluginsManagementView query={query} refreshVersion={refreshVersion} />
        ) : (
          <McpPluginsManagementView query={query} refreshVersion={refreshVersion} />
        )}
      </div>
      <AddPluginMarketplaceDialog
        onClose={() => setMarketplaceOpen(false)}
        onSubmit={submitMarketplace}
        open={marketplaceOpen}
      />
    </div>
  );
}

function findSkillCreator(skills: SkillSummary[]) {
  return skills.find((skill) => skill.name === "skill-creator" || skill.name === "skill_creator");
}

function toSuggestion(skill: SkillSummary): SkillSuggestion {
  return {
    name: skill.name,
    description: skill.description,
    short_description: skill.short_description,
    root: skill.root,
    path: skill.path,
    scope: skill.scope,
    source_plugin: skill.source_plugin,
    source_install: skill.source_install,
  };
}
