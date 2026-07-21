import { useTranslation } from "react-i18next";

import type { ComposerCommand } from "./composer-commands";
import { cn } from "@/shared/lib/cn";

interface ComposerCommandSuggestionsProps {
  commands: ComposerCommand[];
  onHover: (index: number) => void;
  onSelect: (command: ComposerCommand) => void;
  selectedIndex: number;
}

export function ComposerCommandSuggestions({
  commands,
  onHover,
  onSelect,
  selectedIndex,
}: ComposerCommandSuggestionsProps) {
  const { t } = useTranslation(["chat"]);
  if (commands.length === 0) {
    return null;
  }

  return (
    <div
      className="absolute bottom-[calc(100%+8px)] left-3 right-3 z-50 overflow-hidden rounded-xl border border-border/60 bg-card py-1 shadow-lg"
      data-testid="command-suggestions"
    >
      {commands.map((command, index) => (
        <button
          className={cn(
            "flex w-full items-start gap-3 px-3 py-2 text-left outline-none transition focus:outline-none",
            index === selectedIndex ? "bg-muted/15" : "hover:bg-muted/10",
          )}
          key={command}
          onMouseDown={(event) => {
            event.preventDefault();
            onSelect(command);
          }}
          onMouseEnter={() => onHover(index)}
          type="button"
        >
          <span className="text-[13px] font-medium text-ink">/{command}</span>
          <span className="text-[12px] text-muted">
            {t(`chat:command${command[0].toUpperCase()}${command.slice(1)}Description`)}
          </span>
        </button>
      ))}
    </div>
  );
}
