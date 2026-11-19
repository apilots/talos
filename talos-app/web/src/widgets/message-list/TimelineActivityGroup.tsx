import { Brain, ChevronDown, Wrench } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";
import { TimelineRailRow } from "./TimelineRailRow";
import { TimelineStep } from "./TimelineStep";
import { TIMELINE_PAGE_SIZE, type TimelineActivity } from "./timelineDisplay";
import { timelineToolIcon, timelineToolTitle } from "./timelineToolPresentation";
import { toolActivity } from "./toolDisplay";

export function TimelineActivityGroup({ group, timelineId }: { group: TimelineActivity; timelineId: string }) {
  const { t, i18n } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const [visibleCount, setVisibleCount] = useState(TIMELINE_PAGE_SIZE);
  const start = Math.max(0, group.items.length - visibleCount);
  const tools = group.items.filter((item) => item.kind === "tool_call");
  const activities = new Set(tools.map((tool) => toolActivity(tool.toolName)));
  const labels = Array.from(activities).flatMap((activity) => activity ? [t(`timelineActivity.${activity}`)] : []);
  if (labels.length > 0 && activities.has(undefined)) {
    labels.push(t("timelineActivity.other"));
  }
  const singleTool = group.items.length === 1 ? tools[0] : undefined;
  const title = singleTool
    ? timelineToolTitle(singleTool.toolName, singleTool.arguments, t)
    : labels.length
    ? new Intl.ListFormat(i18n.resolvedLanguage).format(labels)
    : t(tools.length ? "timelineToolDetails" : "timelineThinkingDetails");

  return (
    <TimelineRailRow icon={singleTool ? timelineToolIcon(singleTool.toolName) : tools.length ? Wrench : Brain}>
      <details
        className="group/activity"
        data-timeline-activity="true"
        onToggle={(event) => setOpen(event.currentTarget.open)}
      >
        <summary className="block w-fit max-w-full cursor-pointer list-none rounded-md px-1 py-1.5 text-[13px] text-muted marker:hidden hover:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand">
          <span className="flex min-w-0 items-center gap-1.5">
            <span className="truncate">{title}</span>
            <ChevronDown aria-hidden="true" className="shrink-0 group-open/activity:rotate-180" size={13} />
          </span>
        </summary>
        {open ? (
          <div className="space-y-1">
            {start > 0 ? (
              <Button type="button" variant="ghost" onClick={() => setVisibleCount((count) => count + TIMELINE_PAGE_SIZE)}>
                {t("showEarlierSteps", { count: Math.min(TIMELINE_PAGE_SIZE, start) })}
              </Button>
            ) : null}
            <ol className="space-y-0.5">
              {group.items.slice(start).map((item) => {
                const itemKey = `${timelineId}:${item.itemId}`;
                return <li key={itemKey}><TimelineStep grouped={tools.length > 0} item={item} itemKey={itemKey} /></li>;
              })}
            </ol>
          </div>
        ) : null}
      </details>
    </TimelineRailRow>
  );
}
