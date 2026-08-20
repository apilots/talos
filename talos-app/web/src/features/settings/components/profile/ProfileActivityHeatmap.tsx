import { useState } from "react";

import { cn } from "@/shared/lib/cn";

import {
  CumulativeActivityChart,
  DailyActivityChart,
  WeeklyActivityChart,
} from "./ProfileActivityCharts";
import { createActivitySeries, type ProfileActivityPoint } from "./profileActivitySeries";

type ActivityMode = "daily" | "weekly" | "cumulative";

interface ProfileActivityHeatmapProps {
  activity: ProfileActivityPoint[];
  asOfDate: string;
  cumulativeLabel: string;
  dailyLabel: string;
  title: string;
  weeklyLabel: string;
}

const activityModes: ActivityMode[] = ["daily", "weekly", "cumulative"];

export function ProfileActivityHeatmap({
  activity,
  asOfDate,
  cumulativeLabel,
  dailyLabel,
  title,
  weeklyLabel,
}: ProfileActivityHeatmapProps) {
  const [mode, setMode] = useState<ActivityMode>("daily");
  const series = createActivitySeries(activity, asOfDate);
  const labels: Record<ActivityMode, string> = {
    daily: dailyLabel,
    weekly: weeklyLabel,
    cumulative: cumulativeLabel,
  };

  return (
    <section className="mx-auto mt-10 max-w-[760px]">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-4">
        <h3 className="text-sm font-semibold text-ink">{title}</h3>
        <div
          aria-label={title}
          className="inline-flex rounded-[var(--radius-control)] border border-border/80 bg-card/60 p-1 text-xs text-muted"
          role="group"
        >
          {activityModes.map((activityMode) => (
            <button
              aria-pressed={mode === activityMode}
              className={cn(
                "rounded-[calc(var(--radius-control)-0.25rem)] px-4 py-1 font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30",
                mode === activityMode
                  ? "bg-background text-brand shadow-sm"
                  : "hover:text-ink",
              )}
              key={activityMode}
              onClick={() => setMode(activityMode)}
              type="button"
            >
              {labels[activityMode]}
            </button>
          ))}
        </div>
      </div>

      <div className="min-h-[148px]">
        {mode === "daily" ? (
          <DailyActivityChart ariaLabel={`${dailyLabel} ${lowercaseFirst(title)}`} series={series.daily} />
        ) : null}
        {mode === "weekly" ? (
          <WeeklyActivityChart ariaLabel={`${weeklyLabel} ${lowercaseFirst(title)}`} series={series.weekly} />
        ) : null}
        {mode === "cumulative" ? (
          <CumulativeActivityChart ariaLabel={`${cumulativeLabel} ${lowercaseFirst(title)}`} series={series.cumulative} />
        ) : null}
      </div>

      <div className="mt-3 grid grid-cols-12 text-xs text-muted">
        {series.months.map((month, index) => (
          <span key={`${month}-${index}`}>{month}</span>
        ))}
      </div>
    </section>
  );
}

function lowercaseFirst(value: string) {
  return `${value.charAt(0).toLocaleLowerCase()}${value.slice(1)}`;
}
