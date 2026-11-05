import { useId } from "react";
import { useTranslation } from "react-i18next";
import { CircleX } from "lucide-react";
import type { AutomationMode } from "@talos/sdk-browser";
import { AutomationModeIcon } from "@/shared/ui/AutomationModeIcon";

export function ComposerAutomationMode({ mode, onClear }: { mode: AutomationMode; onClear: () => void }) {
  const { t } = useTranslation("chat");
  const modeTooltipId = useId();
  return (
    <span className="group/automation-mode relative ml-1 inline-flex items-center border-l border-border/40 pl-2" data-testid="automation-mode-chip">
      <button type="button" aria-label={t("clearAutomationMode")} aria-describedby={modeTooltipId} className="inline-flex items-center gap-1.5 rounded-sm py-1 text-xs text-muted hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/25" onClick={onClear}>
        <AutomationModeIcon mode={mode} size={14} className="group-hover/automation-mode:hidden group-focus-within/automation-mode:hidden" />
        <CircleX size={14} strokeWidth={1.3} aria-hidden="true" className="hidden group-hover/automation-mode:block group-focus-within/automation-mode:block" />
        {mode === "goal" ? "Goal" : "Loop"}
      </button>
      <span id={modeTooltipId} role="tooltip" className="pointer-events-none invisible absolute bottom-[calc(100%+8px)] left-1/2 z-50 -translate-x-1/2 whitespace-nowrap rounded-md bg-ink px-2 py-1 text-[11px] text-card opacity-0 transition-opacity group-hover/automation-mode:visible group-hover/automation-mode:opacity-100 group-focus-within/automation-mode:visible group-focus-within/automation-mode:opacity-100">{t("automation:clearTitle", { mode: mode === "goal" ? "Goal" : "Loop" })}</span>
    </span>
  );
}
