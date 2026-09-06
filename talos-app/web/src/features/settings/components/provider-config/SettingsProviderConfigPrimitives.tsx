import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";

export type SettingsProviderTabOption<TKey extends string> = {
  key: TKey;
  label: string;
};

type SettingsProviderTabsProps<TKey extends string> = {
  providers: Array<SettingsProviderTabOption<TKey>>;
  activeProvider: TKey;
  onChange: (provider: TKey) => void;
};

export function SettingsProviderTabs<TKey extends string>({
  providers,
  activeProvider,
  onChange,
}: SettingsProviderTabsProps<TKey>) {
  return (
    <div className="mb-5 flex gap-1 rounded-[1rem] border border-border/70 bg-background/40 p-1">
      {providers.map(({ key, label }) => {
        const isActive = activeProvider === key;
        return (
          <button
            className={[
              "flex flex-1 items-center justify-center gap-1.5 rounded-[0.75rem] px-3 py-2 text-sm font-medium transition",
              isActive ? "bg-card text-ink shadow-sm" : "text-muted hover:text-ink",
            ].join(" ")}
            key={key}
            onClick={() => onChange(key)}
            type="button"
          >
            {label}
          </button>
        );
      })}
    </div>
  );
}

type SettingsProviderKeyFieldProps = {
  label: string;
  value: string;
  configured: boolean;
  clearLabel: string;
  onChange: (value: string) => void;
  onClear: () => void;
};

export function SettingsProviderKeyField({
  label,
  value,
  configured,
  clearLabel,
  onChange,
  onClear,
}: SettingsProviderKeyFieldProps) {
  return (
    <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
      <span className="w-24 shrink-0 text-sm text-muted">{label}</span>
      <Input
        aria-label={label}
        className="flex-1 text-sm font-mono"
        onChange={(event) => onChange(event.target.value)}
        type="password"
        value={value}
      />
      {configured ? (
        <Button onClick={onClear} type="button" variant="ghost">
          {clearLabel}
        </Button>
      ) : null}
    </div>
  );
}

type SettingsConfigSaveFooterProps = {
  dirty: boolean | null;
  pending: boolean;
  error: unknown;
  unsavedLabel: string;
  savingLabel: string;
  saveLabel: string;
  onSave: () => void;
  children?: ReactNode;
};

export function SettingsConfigSaveFooter({
  dirty,
  pending,
  error,
  unsavedLabel,
  savingLabel,
  saveLabel,
  onSave,
  children,
}: SettingsConfigSaveFooterProps) {
  useTranslation("errors");
  return (
    <div className="mt-6 flex items-center justify-between border-t border-border/60 pt-4">
      {error ? <span className="text-sm text-danger">{userVisibleErrorMessage(error, "Request failed.")}</span> : null}
      {!error && dirty ? <span className="text-sm text-muted">{unsavedLabel}</span> : null}
      {!error && !dirty ? <span>{children}</span> : null}
      <Button disabled={pending || !dirty} onClick={onSave} type="button">
        {pending ? savingLabel : saveLabel}
      </Button>
    </div>
  );
}
