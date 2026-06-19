import { Box, Check } from "lucide-react";
import type { ProtocolSkillScope, ProtocolSkillSummary } from "@talos/sdk-browser";

interface SkillsCatalogListProps {
  loading: boolean;
  onSkillOpen: (skill: ProtocolSkillSummary) => void;
  skills: ProtocolSkillSummary[];
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
            {items.map((skill) => (
              <SkillRow key={skill.path} onOpen={onSkillOpen} skill={skill} />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

function SkillRow({ onOpen, skill }: { onOpen: (skill: ProtocolSkillSummary) => void; skill: ProtocolSkillSummary }) {
  return (
    <button
      className="grid w-full grid-cols-[44px_minmax(0,1fr)_32px] items-center gap-3 rounded-[0.8rem] px-2 py-2.5 text-left transition hover:bg-card/75 focus:outline-none focus:ring-2 focus:ring-brand/30"
      onClick={() => onOpen(skill)}
      type="button"
    >
      <div className="flex size-10 items-center justify-center rounded-[0.7rem] bg-card text-brand">
        <Box size={18} />
      </div>
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold text-ink">
          {skill.metadata?.display_name ?? titleFromName(skill.name)}
        </div>
        <div className="mt-0.5 truncate text-sm text-muted">
          {skill.short_description ?? skill.description}
        </div>
      </div>
      {skill.enabled ? <Check className="text-muted" size={16} /> : <span className="text-xs text-muted">off</span>}
    </button>
  );
}

function groupSkills(skills: ProtocolSkillSummary[]) {
  const groups = new Map<ProtocolSkillScope, ProtocolSkillSummary[]>();
  for (const skill of skills) {
    const items = groups.get(skill.scope) ?? [];
    items.push(skill);
    groups.set(skill.scope, items);
  }
  return Array.from(groups.entries());
}

function titleFromName(name: string) {
  return name
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}
