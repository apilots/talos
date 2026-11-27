import type { ReactNode } from "react";

import { Panel } from "@/shared/ui/Panel";

export function OverviewCard({
  title,
  value,
  detail,
}: {
  title: string;
  value: string;
  detail: string;
}) {
  return (
    <Panel className="rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] p-4">
      <div className="text-[10px] uppercase tracking-[0.18em] text-muted">{title}</div>
      <div className="mt-2 text-[1.65rem] font-display tracking-[-0.04em] text-ink">{value}</div>
      <p className="mt-1.5 text-sm leading-6 text-muted">{detail}</p>
    </Panel>
  );
}

export function SectionHeader({
  icon,
  title,
  subtitle,
}: {
  icon: ReactNode;
  title: string;
  subtitle: string;
}) {
  return (
    <div className="flex items-start gap-4">
      <div className="flex size-11 items-center justify-center rounded-[var(--radius-control)] border border-border bg-background/55 text-brand">
        {icon}
      </div>
      <div className="flex-1">
        <div className="text-sm font-medium text-ink">{title}</div>
        <p className="mt-1 text-sm text-muted">{subtitle}</p>
      </div>
    </div>
  );
}

export function FieldBlock({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div>
      <label className="mb-2 block text-sm text-muted">{label}</label>
      {children}
    </div>
  );
}
