import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { FileText, Lock } from "lucide-react";
import type { SkillSummary } from "@talos/sdk-browser";

import { listSkills } from "@/features/plugins/api";
import { skillTitle } from "@/features/plugins/lib/pluginDisplay";
import { PluginsManagementView } from "./PluginsManagementView";

interface SkillPluginsManagementViewProps {
  query: string;
  refreshVersion: number;
}

export function SkillPluginsManagementView({ query, refreshVersion }: SkillPluginsManagementViewProps) {
  const availableSkillsQuery = useQuery({
    queryKey: ["talos", "skills", "available", refreshVersion],
    queryFn: () => listSkills(true),
  });
  const availableSkills = useMemo(
    () => filterSkills(
      (availableSkillsQuery.data?.data ?? []).filter((skill) => !skill.source_plugin),
      query,
    ),
    [availableSkillsQuery.data?.data, query],
  );

  return (
    <PluginsManagementView
      catalogLabel="Marketplace skill plugins"
      emptyInstalledDescription="Install a skill plugin from the marketplace to manage it here."
      installedLabel="Installed skill plugins"
      introSection={
        <AvailableSkillsSection
          loading={availableSkillsQuery.isLoading}
          query={query}
          skills={availableSkills}
        />
      }
      kind="skill"
      query={query}
      refreshVersion={refreshVersion}
    />
  );
}

interface AvailableSkillsSectionProps {
  loading: boolean;
  query: string;
  skills: SkillSummary[];
}

function AvailableSkillsSection({ loading, query, skills }: AvailableSkillsSectionProps) {
  return (
    <section>
      <div className="mb-3 flex items-center justify-between gap-3">
        <h2 className="text-sm font-semibold text-ink">Extension skills</h2>
        <span className="rounded-full border border-border/80 px-2 py-1 text-xs text-muted">
          {skills.length}
        </span>
      </div>
      {loading ? (
        <div className="rounded-[1.25rem] border border-dashed border-border/80 bg-card/55 px-6 py-8 text-sm text-muted">
          Loading skills...
        </div>
      ) : skills.length === 0 ? (
        <div className="rounded-[1.25rem] border border-dashed border-border/80 bg-card/55 px-6 py-8 text-sm text-muted">
          {query.trim() ? "No extension skills matched your search." : "No extension skills found."}
        </div>
      ) : (
        <div className="space-y-1">
          {skills.map((skill) => (
            <AvailableSkillRow key={`${skill.path}:${skill.name}`} skill={skill} />
          ))}
        </div>
      )}
    </section>
  );
}

function AvailableSkillRow({ skill }: { skill: SkillSummary }) {
  return (
    <div className="grid w-full grid-cols-[44px_minmax(0,1fr)_max-content] items-center gap-3 rounded-[0.8rem] px-2 py-2.5">
      <div className="flex size-10 items-center justify-center rounded-[0.7rem] bg-card text-brand">
        <FileText size={18} />
      </div>
      <div className="min-w-0">
        <div className="block max-w-full truncate text-left text-sm font-semibold text-ink">
          {skillTitle(skill)}
        </div>
        <div className="mt-0.5 truncate text-sm text-muted">{skill.description}</div>
        <div className="mt-1 truncate text-xs text-muted/80">{compactHomePath(skill.path)}</div>
      </div>
      <div className="flex items-center gap-1.5">
        {skill.read_only ? (
          <span className="inline-flex items-center gap-1 rounded-full border border-border/80 px-2 py-1 text-xs text-muted">
            <Lock size={12} />
            Read-only
          </span>
        ) : null}
        <span className="rounded-full border border-border/80 px-2 py-1 text-xs text-muted">
          Extension
        </span>
        {!skill.enabled ? (
          <span className="rounded-full border border-border/80 px-2 py-1 text-xs text-muted">
            Disabled
          </span>
        ) : null}
        {skill.source_plugin ? (
          <span className="rounded-full border border-border/80 px-2 py-1 text-xs text-muted">
            Plugin
          </span>
        ) : null}
      </div>
    </div>
  );
}

function filterSkills(skills: SkillSummary[], query: string) {
  const normalized = query.trim().toLowerCase();
  if (!normalized) {
    return skills;
  }
  return skills.filter((skill) =>
    [
      skill.name,
      skill.description,
      skill.short_description,
      skill.metadata?.display_name,
      skill.path,
    ].some((value) => value?.toLowerCase().includes(normalized)),
  );
}

function compactHomePath(path: string) {
  if (typeof window === "undefined") {
    return path;
  }
  return path.replace(/^\/Users\/[^/]+/, "~");
}
