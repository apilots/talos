import type { Plugin, SkillSummary } from "@talos/sdk-browser";

import { LocalSkillRow } from "./LocalSkillRow";
import { MarketplaceSkillRow } from "./MarketplaceSkillRow";

type SkillListItem = SkillSummary | Plugin;

interface SkillsCatalogListProps {
  loading: boolean;
  onSkillOpen: (skill: SkillSummary) => void;
  skills: SkillListItem[];
}

export function SkillsCatalogList({ loading, onSkillOpen, skills }: SkillsCatalogListProps) {
  if (loading) {
    return <div className="mt-10 text-sm text-muted">Loading skills...</div>;
  }
  if (skills.length === 0) {
    return <div className="mt-10 text-sm text-muted">No skills found.</div>;
  }
  return (
    <div className="mt-9 space-y-8">
      {groupSkills(skills).map(([scope, items]) => (
        <section key={scope}>
          <h2 className="mb-3 text-sm font-semibold capitalize text-ink">{scope}</h2>
          <div className="space-y-1">
            {items.map((item) =>
              isPlugin(item) ? (
                <MarketplaceSkillRow key={item.id} plugin={item} />
              ) : (
                <LocalSkillRow key={item.path} onOpen={onSkillOpen} skill={item} />
              ),
            )}
          </div>
        </section>
      ))}
    </div>
  );
}

function groupSkills(skills: SkillListItem[]) {
  const groups = new Map<string, SkillListItem[]>();
  for (const skill of skills) {
    const key = isExtensionSkill(skill) ? "Extensions" : "User";
    const items = groups.get(key) ?? [];
    items.push(skill);
    groups.set(key, items);
  }
  return Array.from(groups.entries());
}

function isExtensionSkill(skill: SkillListItem) {
  if (isPlugin(skill)) {
    return false;
  }
  if (skill.scope === "plugin") {
    return false;
  }
  return skill.read_only;
}

function isPlugin(item: SkillListItem): item is Plugin {
  return "definition" in item;
}
