import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import type { Plugin, SkillSuggestion, SkillSummary } from "@talos/sdk-browser";

import { AddPluginMarketplaceDialog } from "@/features/plugins/components/AddPluginMarketplaceDialog";
import {
  addMarketplaceSource,
  disableSkillPlugin,
  enableSkillPlugin,
  installSkillPlugin,
  listManagedSkillPlugins,
  listSkills,
  searchSkillPlugins,
  uninstallSkillPlugin,
  updateSkillPlugin,
} from "@/features/plugins/api";
import { PluginCatalogActions } from "@/features/plugins/components/PluginCatalogActions";
import { PluginCatalogSearch } from "@/features/plugins/components/PluginCatalogSearch";
import { PluginCatalogTabs } from "@/features/plugins/components/PluginCatalogTabs";
import { PluginDocumentDialog } from "@/features/plugins/components/PluginDocumentDialog";
import { PluginManagementPanel } from "@/features/plugins/components/PluginManagementPanel";
import { SkillDocumentDialog } from "@/features/plugins/components/SkillDocumentDialog";
import { SkillsCatalogList } from "@/features/plugins/components/SkillsCatalogList";
import type { PluginsCatalogTab } from "@/features/plugins/types";

const CREATE_SKILL_DRAFT =
  "I want to create a new Talos skill. Ask me for the skill name, trigger, workflow, and files it should include.";
const CREATE_PLUGIN_DRAFT =
  "I want to create a new Talos plugin. Ask me for the plugin name, capabilities, skill roots, MCP servers, and package layout.";

export function PluginsPage() {
  const navigate = useNavigate();
  const [tab, setTab] = useState<PluginsCatalogTab>("plugins");
  const [query, setQuery] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [marketplaceOpen, setMarketplaceOpen] = useState(false);
  const [selectedSkill, setSelectedSkill] = useState<SkillSummary | null>(null);
  const [selectedPlugin, setSelectedPlugin] = useState<Plugin | null>(null);
  const [actionPluginId, setActionPluginId] = useState<string | null>(null);
  const skillsQuery = useQuery({
    queryKey: ["talos", "skills", "catalog"],
    queryFn: () => listSkills(true),
  });
  const installedPluginsQuery = useQuery({
    enabled: tab === "plugins",
    queryKey: ["talos", "plugins", "installed", "skills"],
    queryFn: listManagedSkillPlugins,
  });
  const marketplaceQuery = useQuery({
    enabled: tab === "plugins",
    queryKey: ["talos", "plugins", "skills", "search", query],
    queryFn: () => searchSkillPlugins(query, "refresh_stale"),
  });
  const skills = useMemo(
    () => filterSkills(skillsQuery.data?.data ?? [], query),
    [query, skillsQuery.data?.data],
  );
  const marketplacePlugins = useMemo(
    () => marketplaceQuery.data?.plugins ?? [],
    [marketplaceQuery.data?.plugins],
  );
  const installedPlugins = useMemo(
    () => installedPluginsQuery.data?.plugins ?? [],
    [installedPluginsQuery.data?.plugins],
  );

  function createSkill() {
    setMenuOpen(false);
    const creator = findSkillCreator(skillsQuery.data?.data ?? []);
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

  async function refresh() {
    if (tab !== "skills") {
      await Promise.all([
        installedPluginsQuery.refetch(),
        searchSkillPlugins(query, "force_refresh").then(() => marketplaceQuery.refetch()),
      ]);
      return;
    }
    await skillsQuery.refetch();
  }

  async function submitMarketplace(input: { source: string; gitRef: string; sparsePaths: string }) {
    await addMarketplaceSource(input);
    void marketplaceQuery.refetch();
  }

  async function runPluginAction(plugin: Plugin, action: (plugin: Plugin) => Promise<unknown>) {
    setActionPluginId(plugin.id);
    try {
      await action(plugin);
      await Promise.all([
        installedPluginsQuery.refetch(),
        marketplaceQuery.refetch(),
        skillsQuery.refetch(),
      ]);
    } finally {
      setActionPluginId(null);
    }
  }

  return (
    <div className="h-full overflow-y-auto bg-background px-8 py-7 text-ink">
      <div className="mx-auto max-w-[920px]">
        <div className="flex items-start justify-between gap-4">
          <div>
            <PluginCatalogTabs activeTab={tab} onTabChange={setTab} />
            <h1 className="font-display text-[34px] font-semibold tracking-tight">
              {tab === "plugins" ? "Plugins" : "Skills"}
            </h1>
            <p className="mt-1 text-[15px] text-muted">
              {tab === "plugins"
                ? "Extend Talos with packaged integrations and reusable capabilities."
                : "Extend Talos with task-specific instruction bundles."}
            </p>
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
          <PluginManagementPanel
            actionPluginId={actionPluginId}
            installedPlugins={installedPlugins}
            loading={installedPluginsQuery.isLoading || marketplaceQuery.isLoading}
            marketplacePlugins={marketplacePlugins}
            onDisable={(plugin) => void runPluginAction(plugin, disableSkillPlugin)}
            onEnable={(plugin) => void runPluginAction(plugin, enableSkillPlugin)}
            onInstall={(plugin) => void runPluginAction(plugin, installSkillPlugin)}
            onOpen={setSelectedPlugin}
            onUninstall={(plugin) => void runPluginAction(plugin, uninstallSkillPlugin)}
            onUpdate={(plugin) => void runPluginAction(plugin, updateSkillPlugin)}
            warnings={warningMessages(marketplaceQuery.data?.warnings ?? [])}
          />
        ) : (
          <SkillsCatalogList
            loading={skillsQuery.isLoading && skills.length === 0}
            onSkillOpen={setSelectedSkill}
            skills={skills}
          />
        )}
      </div>
      <AddPluginMarketplaceDialog
        onClose={() => setMarketplaceOpen(false)}
        onSubmit={submitMarketplace}
        open={marketplaceOpen}
      />
      <PluginDocumentDialog onClose={() => setSelectedPlugin(null)} plugin={selectedPlugin} />
      <SkillDocumentDialog onClose={() => setSelectedSkill(null)} skill={selectedSkill} />
    </div>
  );
}

function filterSkills(skills: SkillSummary[], query: string) {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return skills;
  }
  return skills.filter((skill) =>
    [skill.name, skill.description, skill.short_description ?? "", skill.source_plugin ?? ""]
      .join(" ")
      .toLowerCase()
      .includes(needle),
  );
}

function warningMessages(warnings: Array<{ message: string }>) {
  return warnings.map((warning) => warning.message);
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
