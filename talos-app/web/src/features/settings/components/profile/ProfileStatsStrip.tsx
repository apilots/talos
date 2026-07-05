export interface ProfileStatView {
  label: string;
  value: string;
}

interface ProfileStatsStripProps {
  stats: ProfileStatView[];
}

export function ProfileStatsStrip({ stats }: ProfileStatsStripProps) {
  return (
    <section className="mx-auto mt-12 grid max-w-[760px] grid-cols-2 overflow-hidden rounded-[1.05rem] border border-border/80 bg-card/45 md:grid-cols-5">
      {stats.map((stat) => (
        <div className="border-b border-r border-border/65 px-4 py-3 text-center last:border-r-0 md:border-b-0" key={stat.label}>
          <div className="text-lg font-semibold tabular-nums text-ink">{stat.value}</div>
          <div className="mt-1 text-xs text-muted">{stat.label}</div>
        </div>
      ))}
    </section>
  );
}
