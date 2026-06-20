import { useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import type { SkillSuggestion, SkillSummary } from "@talos/sdk-browser";

import { AddPluginMarketplaceDialog } from "@/features/plugins/components/AddPluginMarketplaceDialog";
import { AllPluginsManagementView } from "@/features/plugins/components/AllPluginsManagementView";
import { McpPluginsManagementView } from "@/features/plugins/components/McpPluginsManagementView";
import { PluginCatalogActions } from "@/features/plugins/components/PluginCatalogActions";
import { PluginCatalogSearch } from "@/features/plugins/components/PluginCatalogSearch";
import { PluginCatalogTabs } from "@/features/plugins/components/PluginCatalogTabs";
import { SkillPluginsManagementView } from "@/features/plugins/components/SkillPluginsManagementView";
import { addMarketplaceSource, listSkills } from "@/features/plugins/api";
import type { PluginsCatalogTab } from "@/features/plugins/types";

const CREATE_SKILL_DRAFT =
  "I want to create a new Talos skill. Ask me for the skill name, trigger, workflow, and files it should include.";
const CREATE_PLUGIN_DRAFT =
  "I want to create a new Talos plugin. Ask me for the plugin name, capabilities, skill roots, MCP servers, and package layout.";

export function PluginsPage() {
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
        initialDraft: creator ? "" : CREATE_SKILL_DRAFT,
        initialSkills: creator ? [toSuggestion(creator)] : [],
      },
    });
  }

  function createPlugin() {
    setMenuOpen(false);
    navigate("/", { state: { initialDraft: CREATE_PLUGIN_DRAFT } });
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
            <h1 className="font-display text-[34px] font-semibold tracking-tight">{pageTitle(tab)}</h1>
            <p className="mt-1 text-[15px] text-muted">{pageDescription(tab)}</p>
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

function pageTitle(tab: PluginsCatalogTab) {
  switch (tab) {
    case "skills":
      return "Skills";
    case "mcp":
      return "MCP";
    case "plugins":
      return "Plugins";
  }
}

function pageDescription(tab: PluginsCatalogTab) {
  switch (tab) {
    case "skills":
      return "Manage skill plugins that provide task-specific instruction bundles.";
    case "mcp":
      return "Manage MCP server plugins that expose model-callable tools.";
    case "plugins":
      return "Manage all installed and marketplace plugin capabilities.";
  }
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
