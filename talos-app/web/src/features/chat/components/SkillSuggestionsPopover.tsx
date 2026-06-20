import { Package } from "lucide-react";
import type { SkillSuggestion } from "@talos/sdk-browser";

import { cn } from "@/shared/lib/cn";

interface SkillSuggestionsPopoverProps {
  suggestions: SkillSuggestion[];
  selectedIndex: number;
  onHover: (index: number) => void;
  onSelect: (skill: SkillSuggestion) => void;
}

export function SkillSuggestionsPopover({
  suggestions,
  selectedIndex,
  onHover,
  onSelect,
}: SkillSuggestionsPopoverProps) {
  if (suggestions.length === 0) {
    return null;
  }

  return (
    <div
      className="absolute bottom-[calc(100%+8px)] left-0 right-0 z-50 mx-auto max-h-72 w-[min(880px,calc(100%-1rem))] overflow-hidden rounded-[1.4rem] border border-border/70 bg-card px-2 py-2 shadow-2xl"
      data-testid="skill-suggestions"
    >
      <div className="max-h-64 overflow-y-auto">
        {suggestions.map((skill, index) => (
          <button
            className={cn(
              "grid w-full grid-cols-[1rem_minmax(9rem,auto)_minmax(0,1fr)_auto] items-center gap-3 rounded-2xl px-3 py-2.5 text-left outline-none transition",
              index === selectedIndex ? "bg-muted/20" : "hover:bg-muted/15",
            )}
            key={skill.path}
            onMouseEnter={() => onHover(index)}
            onMouseDown={(event) => {
              event.preventDefault();
              onSelect(skill);
            }}
            type="button"
          >
            <Package size={15} className="text-muted" strokeWidth={1.8} />
            <span className="truncate text-[15px] font-medium leading-5 text-ink">
              {skillDisplayName(skill.name)}
            </span>
            <span className="truncate text-[14px] leading-5 text-muted">{skill.description}</span>
            <span className="shrink-0 text-[13px] leading-5 text-muted">
              {skillSourceLabel(skill.scope)}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

function skillDisplayName(name: string) {
  return name
    .split(/[-_:]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function skillSourceLabel(scope: SkillSuggestion["scope"]) {
  switch (scope) {
    case "external":
      return "External";
    case "project":
      return "Project";
    case "plugin":
      return "Plugin";
    case "system":
      return "System";
    default:
      return "Skill";
  }
}
