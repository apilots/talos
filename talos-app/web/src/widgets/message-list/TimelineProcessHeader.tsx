import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useNowMs } from "@/shared/hooks/useNowMs";
import { cn } from "@/shared/lib/cn";
import { formatDuration } from "@/shared/lib/formatDuration";
import type { timelineProcessTiming } from "./timelineDisplay";

export function TimelineProcessHeader({
  timing,
  live,
  paused,
  expanded,
  onToggle,
}: {
  timing: ReturnType<typeof timelineProcessTiming>;
  live: boolean;
  paused: boolean;
  expanded: boolean;
  onToggle?: () => void;
}) {
  const { t } = useTranslation("chat");
  const { startedAtMs, durationMs, outcome } = timing;
  const now = useNowMs(live && !paused && startedAtMs !== null);
  const elapsed = live && startedAtMs !== null ? Math.max(0, now - startedAtMs) : durationMs;
  const state = live ? (paused ? "processElapsed" : "processWorking") : outcome;
  const label = elapsed === null
    ? t(state)
    : t(`${state}Duration`, { duration: formatDuration(elapsed) });
  const className = "flex items-center gap-1.5 rounded-sm px-1 py-2 text-[13px] tabular-nums text-muted";

  if (!onToggle) return <p className={className}>{label}</p>;
  return (
    <button
      aria-expanded={expanded}
      className={cn(className, "hover:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand")}
      data-timeline-overview="true"
      onClick={onToggle}
      type="button"
    >
      {label}
      <ChevronDown aria-hidden="true" className={expanded ? "rotate-180" : undefined} size={13} />
    </button>
  );
}
