import { Box, Check } from "lucide-react";
import type { ProtocolSkillSummary } from "@talos/sdk-browser";

interface LocalSkillRowProps {
  skill: ProtocolSkillSummary;
  onOpen: (skill: ProtocolSkillSummary) => void;
}

export function LocalSkillRow({ skill, onOpen }: LocalSkillRowProps) {
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
      {skill.enabled ? (
        <Check className="text-muted" size={16} />
      ) : (
        <span className="text-xs text-muted">off</span>
      )}
    </button>
  );
}

function titleFromName(name: string) {
  return name
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}
