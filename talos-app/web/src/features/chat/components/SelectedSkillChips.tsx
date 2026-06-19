import { X } from "lucide-react";
import type { ProtocolSkillSuggestion } from "@talos/sdk-browser";

interface SelectedSkillChipsProps {
  draft: string;
  skills: ProtocolSkillSuggestion[];
  onDraftChange: (value: string) => void;
  onSkillsChange: (skills: ProtocolSkillSuggestion[]) => void;
}

export function SelectedSkillChips({
  draft,
  skills,
  onDraftChange,
  onSkillsChange,
}: SelectedSkillChipsProps) {
  if (skills.length === 0) {
    return null;
  }
  return (
    <div className="flex flex-wrap gap-1.5 px-4 pt-3">
      {skills.map((skill) => (
        <span
          className="inline-flex max-w-full items-center gap-1.5 rounded-full border border-brand/25 bg-brand/8 px-2.5 py-1 text-[12px] font-medium text-brand"
          key={skill.path}
        >
          <span className="truncate">${skill.name}</span>
          <button
            aria-label={`Remove ${skill.name}`}
            className="rounded-full p-0.5 text-brand/70 transition hover:bg-brand/10 hover:text-brand"
            onClick={() => {
              onSkillsChange(skills.filter((item) => item.path !== skill.path));
              onDraftChange(draft.replaceAll(`$${skill.name}`, "").replace(/\s{2,}/g, " "));
            }}
            type="button"
          >
            <X size={12} />
          </button>
        </span>
      ))}
    </div>
  );
}
