import { cn } from "@/shared/lib/cn";

export interface ProfileActivityPoint {
  timestamp: number;
  tokens: number;
}

interface ProfileActivityHeatmapProps {
  activity: ProfileActivityPoint[];
  cumulativeLabel: string;
  dailyLabel: string;
  title: string;
  weeklyLabel: string;
}

export function ProfileActivityHeatmap({
  activity,
  cumulativeLabel,
  dailyLabel,
  title,
  weeklyLabel,
}: ProfileActivityHeatmapProps) {
  const { cells, months } = createActivityHeatmap(activity);

  return (
    <section className="mx-auto mt-10 max-w-[760px]">
      <div className="mb-4 flex items-center justify-between gap-4">
        <h3 className="text-sm font-semibold text-ink">{title}</h3>
        <div className="inline-flex rounded-[var(--radius-control)] border border-border/80 bg-card/60 p-1 text-xs text-muted">
          <span className="rounded-[calc(var(--radius-control)-0.25rem)] bg-background px-4 py-1 font-medium text-brand shadow-sm">
            {dailyLabel}
          </span>
          <span className="rounded-[calc(var(--radius-control)-0.25rem)] px-4 py-1">
            {weeklyLabel}
          </span>
          <span className="rounded-[calc(var(--radius-control)-0.25rem)] px-4 py-1">
            {cumulativeLabel}
          </span>
        </div>
      </div>
      <div className="grid grid-flow-col grid-rows-7 gap-1">
        {cells.map((level, index) => (
          <span
            aria-hidden="true"
            className={cn(
              "size-2.5 rounded-[0.18rem]",
              level === 0 && "bg-[#f3f4f6]",
              level === 1 && "bg-[#d8ebff]",
              level === 2 && "bg-[#afd8ff]",
              level === 3 && "bg-[#6db6ff]",
              level >= 4 && "bg-[#1f8fff]",
            )}
            key={index}
          />
        ))}
      </div>
      <div className="mt-3 grid grid-cols-12 text-xs text-muted">
        {months.map((month, index) => (
          <span key={`${month}-${index}`}>{month}</span>
        ))}
      </div>
    </section>
  );
}

function createActivityHeatmap(activity: ProfileActivityPoint[]) {
  const today = startOfLocalDay(new Date());
  const start = new Date(today);
  start.setDate(today.getDate() - (52 * 7 - 1));
  const tokensByDay = new Map<number, number>();

  for (const point of activity) {
    const date = startOfLocalDay(new Date(point.timestamp * 1000));
    if (date < start || date > today) {
      continue;
    }
    const dayKey = date.getTime();
    tokensByDay.set(dayKey, (tokensByDay.get(dayKey) ?? 0) + point.tokens);
  }

  const maxTokens = Math.max(0, ...tokensByDay.values());
  const cells = Array.from({ length: 52 * 7 }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    return tokenLevel(tokensByDay.get(date.getTime()) ?? 0, maxTokens);
  });

  const months = Array.from({ length: 12 }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + Math.floor(index * ((52 * 7) / 12)));
    return new Intl.DateTimeFormat("en-US", { month: "short" }).format(date);
  });

  return { cells, months };
}

function tokenLevel(tokens: number, maxTokens: number) {
  if (tokens <= 0 || maxTokens <= 0) {
    return 0;
  }
  const ratio = tokens / maxTokens;
  if (ratio >= 0.75) {
    return 4;
  }
  if (ratio >= 0.5) {
    return 3;
  }
  if (ratio >= 0.25) {
    return 2;
  }
  return 1;
}

function startOfLocalDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}
