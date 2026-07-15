import { Plus, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

export function FormSection({ children, title }: { children: ReactNode; title: string }) {
  return (
    <div className="space-y-3 border-b border-border/45 px-5 py-4 last:border-b-0">
      <h3 className="text-sm font-semibold text-ink">{title}</h3>
      {children}
    </div>
  );
}

export function InlineInputRow({ children }: { children: ReactNode }) {
  return <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_2rem] items-center gap-3">{children}</div>;
}

export function SingleInputRow({ children }: { children: ReactNode }) {
  return <div className="grid grid-cols-[minmax(0,1fr)_2rem] items-center gap-3">{children}</div>;
}

export function TextInput({
  defaultValue,
  placeholder,
  readOnly = false,
}: {
  defaultValue?: string;
  placeholder?: string;
  readOnly?: boolean;
}) {
  return (
    <input
      className="form-control-focus h-10 w-full rounded-[0.75rem] border border-border/75 bg-background/60 px-3 text-sm font-medium text-ink placeholder:text-muted/70 read-only:text-muted"
      defaultValue={defaultValue}
      placeholder={placeholder}
      readOnly={readOnly}
      type="text"
    />
  );
}

export function DisabledAddButton({ label }: { label: string }) {
  const { t } = useTranslation("settings");

  return (
    <button
      className="inline-flex h-10 w-full cursor-not-allowed items-center justify-center gap-2 rounded-[0.75rem] bg-muted/10 text-sm font-medium text-muted"
      disabled
      title={t("mcpServers.savePendingApi")}
      type="button"
    >
      <Plus aria-hidden="true" size={16} />
      {label}
    </button>
  );
}

export function TrashIconButton() {
  return (
    <button
      className="flex size-8 cursor-not-allowed items-center justify-center rounded-[var(--radius-control)] text-muted/45"
      disabled
      type="button"
    >
      <Trash2 aria-hidden="true" size={15} />
    </button>
  );
}

export function firstEntry(record: Record<string, string | undefined> | undefined): [string, string] | null {
  const entry = Object.entries(record ?? {}).find(([, value]) => value !== undefined);
  return entry && entry[1] !== undefined ? [entry[0], entry[1]] : null;
}
