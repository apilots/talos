import type { ReactNode } from "react";

export interface ProfileMetricView {
  icon: ReactNode;
  label: string;
  value: string;
}

interface ProfileInsightsGridProps {
  leftTitle: string;
  leftRows: ProfileMetricView[];
  rightTitle: string;
  rightRows: ProfileMetricView[];
}

export function ProfileInsightsGrid({
  leftRows,
  leftTitle,
  rightRows,
  rightTitle,
}: ProfileInsightsGridProps) {
  return (
    <div className="mx-auto mt-12 grid max-w-[760px] gap-12 lg:grid-cols-2">
      <ProfileMetricList rows={leftRows} title={leftTitle} />
      <ProfileMetricList rows={rightRows} title={rightTitle} />
    </div>
  );
}

function ProfileMetricList({
  rows,
  title,
}: {
  rows: ProfileMetricView[];
  title: string;
}) {
  return (
    <section>
      <h3 className="mb-4 text-sm font-semibold text-ink">{title}</h3>
      {rows.map((row) => (
        <div className="grid grid-cols-[1.5rem_minmax(0,1fr)_max-content] items-center gap-3 border-b border-border/70 py-3 last:border-b-0" key={row.label}>
          <span className="text-muted">{row.icon}</span>
          <span className="min-w-0 truncate text-sm text-muted">{row.label}</span>
          <span className="text-sm font-medium tabular-nums text-ink">{row.value}</span>
        </div>
      ))}
    </section>
  );
}
