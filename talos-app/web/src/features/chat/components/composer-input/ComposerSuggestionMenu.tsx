import { Fragment, useLayoutEffect, useRef } from "react";
import { Box, ChartNoAxesColumn, Eraser, Gauge, Package, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import { contextUsedPercent, type ContextUsage } from "../contextUsage";
import type { ComposerCommandId, ComposerSuggestion } from "./composer-suggestions";

interface ComposerSuggestionMenuProps {
  contextUsage?: ContextUsage;
  error?: string;
  loading: boolean;
  onHover: (index: number) => void;
  onSelect: (suggestion: ComposerSuggestion) => void;
  open: boolean;
  selectedIndex: number;
  suggestions: ComposerSuggestion[];
}

const COMMAND_ICONS: Record<ComposerCommandId, LucideIcon> = {
  compact: Box,
  clear: Eraser,
  status: Gauge,
  model: ChartNoAxesColumn,
};

export function ComposerSuggestionMenu({
  contextUsage,
  error,
  loading,
  onHover,
  onSelect,
  open,
  selectedIndex,
  suggestions,
}: ComposerSuggestionMenuProps) {
  const { t } = useTranslation("chat");
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);

  useLayoutEffect(() => {
    if (!open) {
      return;
    }
    optionRefs.current[selectedIndex]?.scrollIntoView({
      block: "nearest",
      inline: "nearest",
    });
  }, [open, selectedIndex, suggestions]);

  if (!open) {
    return null;
  }

  const commandCount = suggestions.filter((suggestion) => suggestion.type === "command").length;
  const hasSkills = suggestions.some((suggestion) => suggestion.type === "skill");

  return (
    <div
      aria-label={t("composerSuggestions")}
      className={cn(
        "absolute bottom-[calc(100%+10px)] left-0 right-0 z-50 mx-auto max-h-[min(520px,42vh)] w-full overflow-hidden rounded-[1.35rem] border border-border/70 bg-card px-2 py-2 shadow-[0_24px_64px_rgba(20,24,32,0.16)]",
        loading && "min-h-[min(420px,42vh)]",
      )}
      data-testid="composer-suggestions"
      role="listbox"
    >
      <div className="max-h-[min(500px,40vh)] overflow-y-auto overscroll-contain">
        {suggestions.map((suggestion, index) => {
          const selected = index === selectedIndex;
          let description: string;
          let key: string;
          let source: string | undefined;
          let title: string;
          let Icon: LucideIcon = Package;

          if (suggestion.type === "command") {
            Icon = COMMAND_ICONS[suggestion.id];
            key = `command:${suggestion.id}`;
            title = commandTitle(suggestion.id);
            description = suggestion.id === "compact" && contextUsage?.usedTokens
              ? t("commandCompactDescriptionWithUsage", {
                  percent: contextUsedPercent(contextUsage).toFixed(0),
                })
              : t(`command${capitalize(suggestion.id)}Description`);
          } else {
            key = `skill:${suggestion.skill.path}`;
            title = skillDisplayName(suggestion.skill.name, suggestion.skill.metadata?.display_name);
            description = suggestion.skill.short_description ?? suggestion.skill.description;
            source = skillSourceLabel(suggestion.skill.scope);
          }

          return (
            <Fragment key={key}>
              {source && index === commandCount ? (
                <div className="px-2.5 pb-1 pt-2 text-[13px] font-medium text-muted">
                  {t("skillsGroup")}
                </div>
              ) : null}
              <button
                aria-selected={selected}
                className={rowClassName(selected)}
                onMouseDown={(event) => {
                  event.preventDefault();
                  onSelect(suggestion);
                }}
                onMouseMove={() => onHover(index)}
                ref={(element) => {
                  optionRefs.current[index] = element;
                }}
                role="option"
                type="button"
              >
                <Icon className="shrink-0 text-ink/65" size={18} strokeWidth={source ? 1.8 : undefined} />
                <span className="min-w-0 truncate text-[15px] font-medium text-ink/75">
                  {title}
                </span>
                {source ? (
                  <span className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-4">
                    <span className="truncate text-[14px] text-muted">{description}</span>
                    <span className="shrink-0 text-[13px] text-muted">{source}</span>
                  </span>
                ) : (
                  <span className="min-w-0 truncate text-right text-[14px] text-muted">
                    {description}
                  </span>
                )}
              </button>
            </Fragment>
          );
        })}

        {!hasSkills && (loading || error) ? (
          <div className="px-2.5 pb-2 pt-2">
            <div className="mb-1 text-[13px] font-medium text-muted">{t("skillsGroup")}</div>
            <div className={cn("px-8 py-2 text-[13px]", error ? "text-danger" : "text-muted")}>
              {error ?? t("loadingSkills")}
            </div>
          </div>
        ) : null}

        {suggestions.length === 0 && !loading && !error ? (
          <div className="px-5 py-7 text-center text-[14px] text-muted">
            {t("noComposerSuggestions")}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function rowClassName(selected: boolean) {
  return cn(
    "grid min-h-12 w-full grid-cols-[1.25rem_minmax(8rem,auto)_minmax(0,1fr)] items-center gap-3 rounded-2xl px-2.5 py-2 text-left outline-none transition-colors focus:outline-none",
    selected ? "bg-muted/15" : "hover:bg-muted/10",
  );
}

function commandTitle(command: ComposerCommandId) {
  return `${command.charAt(0).toUpperCase()}${command.slice(1)}`;
}

function capitalize(value: string) {
  return `${value.charAt(0).toUpperCase()}${value.slice(1)}`;
}

function skillDisplayName(name: string, configuredName?: string | null) {
  if (configuredName?.trim()) {
    return configuredName.trim();
  }
  return name
    .split(/[-_:]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function skillSourceLabel(scope: "external" | "project" | "plugin" | "system") {
  return scope.charAt(0).toUpperCase() + scope.slice(1);
}
