import { PluginsManagementView } from "./PluginsManagementView";

interface SkillPluginsManagementViewProps {
  query: string;
  refreshVersion: number;
}

export function SkillPluginsManagementView({ query, refreshVersion }: SkillPluginsManagementViewProps) {
  return (
    <PluginsManagementView
      catalogLabel="Marketplace skills"
      emptyInstalledDescription="Add a marketplace source or install a skill plugin to make it available in Skills."
      installedLabel="Installed skills"
      kind="skill"
      query={query}
      refreshVersion={refreshVersion}
    />
  );
}
