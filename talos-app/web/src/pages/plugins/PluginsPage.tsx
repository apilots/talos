import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import type { Plugin, ProtocolSkillSuggestion, ProtocolSkillSummary } from "@talos/sdk-browser";

import { AddPluginMarketplaceDialog } from "@/features/plugins/components/AddPluginMarketplaceDialog";
import { addMarketplaceSource, listSkills, searchSkillPlugins } from "@/features/plugins/api";
import { PluginCatalogActions } from "@/features/plugins/components/PluginCatalogActions";
import { PluginCatalogSearch } from "@/features/plugins/components/PluginCatalogSearch";
import { PluginCatalogTabs } from "@/features/plugins/components/PluginCatalogTabs";
import { PluginsEmptyState } from "@/features/plugins/components/PluginsEmptyState";
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
  const [selectedSkill, setSelectedSkill] = useState<ProtocolSkillSummary | null>(null);
  const skillsQuery = useQuery({
    queryKey: ["talos", "skills", "catalog"],
    queryFn: () => listSkills(true),
  });
  const marketplaceQuery = useQuery({
    enabled: tab === "skills",
    queryKey: ["talos", "plugins", "skills", "search", query],
    queryFn: () => searchSkillPlugins(query),
  });
  const skills = useMemo(
    () => buildSkillItems(skillsQuery.data?.data ?? [], marketplaceQuery.data?.plugins ?? [], query),
    [marketplaceQuery.data?.plugins, query, skillsQuery.data?.data],
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
      return;
    }
    await Promise.all([skillsQuery.refetch(), marketplaceQuery.refetch()]);
  }

  async function submitMarketplace(input: { source: string; gitRef: string; sparsePaths: string }) {
    await addMarketplaceSource(input);
    await marketplaceQuery.refetch();
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
          <PluginsEmptyState />
        ) : (
          <SkillsCatalogList
            loading={skillsQuery.isLoading || marketplaceQuery.isLoading}
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
      <SkillDocumentDialog onClose={() => setSelectedSkill(null)} skill={selectedSkill} />
    </div>
  );
}

function buildSkillItems(
  localSkills: ProtocolSkillSummary[],
  marketplacePlugins: Plugin[],
  query: string,
): Array<ProtocolSkillSummary | Plugin> {
  return [...filterSkills(localSkills, query), ...marketplacePlugins];
}

function filterSkills(skills: ProtocolSkillSummary[], query: string) {
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

function findSkillCreator(skills: ProtocolSkillSummary[]) {
  return skills.find((skill) => skill.name === "skill-creator" || skill.name === "skill_creator");
}

function toSuggestion(skill: ProtocolSkillSummary): ProtocolSkillSuggestion {
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
