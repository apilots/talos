import { cn } from "@/shared/lib/cn";

import type { DatedValue } from "./profileActivitySeries";

export function DailyActivityChart({ ariaLabel, series }: ChartProps) {
  const maxTokens = Math.max(0, ...series.map((point) => point.value));
  return (
    <div aria-label={ariaLabel} className="grid grid-flow-col grid-rows-7 gap-1" role="img">
      {series.map((point) => {
        const level = tokenLevel(point.value, maxTokens);
        return (
          <span
            aria-hidden="true"
            className={cn(
              "size-2.5 rounded-[0.18rem]",
              level === 0 && "bg-activity-empty",
              level === 1 && "bg-activity-1",
              level === 2 && "bg-activity-2",
              level === 3 && "bg-activity-3",
              level >= 4 && "bg-activity-4",
            )}
            key={point.date}
            title={`${point.date}: ${formatTokens(point.value)}`}
          />
        );
      })}
    </div>
  );
}

export function WeeklyActivityChart({ ariaLabel, series }: ChartProps) {
  const maxTokens = Math.max(0, ...series.map((point) => point.value));
  return (
    <div
      aria-label={ariaLabel}
      className="grid items-end gap-1 border-b border-border/70 px-0.5 pt-3"
      role="img"
      style={{ gridTemplateColumns: "repeat(52, minmax(0, 1fr))", height: 148 }}
    >
      {series.map((point) => (
        <span
          aria-hidden="true"
          className="rounded-t-[0.18rem] transition-[height] duration-300"
          key={point.date}
          style={{
            backgroundColor: "rgb(var(--color-brand) / 0.75)",
            height: chartHeight(point.value, maxTokens),
            minHeight: 1,
          }}
          title={`${point.date}: ${formatTokens(point.value)}`}
        />
      ))}
    </div>
  );
}

export function CumulativeActivityChart({ ariaLabel, series }: ChartProps) {
  const width = 728;
  const height = 148;
  const maxTokens = Math.max(0, ...series.map((point) => point.value));
  const points = series.map((point, index) => ({
    x: (index / Math.max(1, series.length - 1)) * width,
    y: height - (point.value / Math.max(1, maxTokens)) * (height - 14),
  }));
  const line = points.map((point) => `${point.x},${point.y}`).join(" ");
  const area = `0,${height} ${line} ${width},${height}`;

  return (
    <svg
      aria-label={ariaLabel}
      className="h-[148px] w-full overflow-visible border-b border-border/70"
      preserveAspectRatio="none"
      role="img"
      viewBox={`0 0 ${width} ${height}`}
    >
      <defs>
        <linearGradient id="profile-activity-fill" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor="rgb(var(--color-brand))" stopOpacity="0.28" />
          <stop offset="100%" stopColor="rgb(var(--color-brand))" stopOpacity="0.02" />
        </linearGradient>
      </defs>
      <polygon fill="url(#profile-activity-fill)" points={area} />
      <polyline
        fill="none"
        points={line}
        stroke="rgb(var(--color-brand))"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2.5"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

interface ChartProps {
  ariaLabel: string;
  series: DatedValue[];
}

function tokenLevel(tokens: number, maxTokens: number) {
  if (tokens <= 0 || maxTokens <= 0) return 0;
  const ratio = tokens / maxTokens;
  if (ratio >= 0.75) return 4;
  if (ratio >= 0.5) return 3;
  if (ratio >= 0.25) return 2;
  return 1;
}

function chartHeight(tokens: number, maxTokens: number) {
  if (tokens <= 0 || maxTokens <= 0) return "1px";
  return `${Math.max(4, (tokens / maxTokens) * 100)}%`;
}

function formatTokens(value: number) {
  return new Intl.NumberFormat("en-US").format(value);
}
