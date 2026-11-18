import { CheckCircle2, ServerCog } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { useCopyText } from "@/shared/hooks/useCopyText";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { isDesktopRuntime } from "@/shared/api/desktop-client";
import { FieldBlock, SectionHeader } from "./SectionPrimitives";

export type SettingsServerConnectionProps = Pick<
  SettingsPageState,
  | "connection"
  | "setConnection"
  | "draft"
  | "setDraft"
  | "models"
  | "setLocale"
>;

export function SettingsServerConnectionPanel({
  connection,
  setConnection,
  draft,
  setDraft,
  models,
  setLocale,
}: SettingsServerConnectionProps) {
  const { t } = useTranslation(["settings", "common"]);
  const desktopRuntime = isDesktopRuntime();
  const hasChanges = draft.authToken !== connection.authToken;

  return (
    <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/[0.88] p-6">
      <SectionHeader
        icon={<ServerCog size={18} />}
        subtitle={desktopRuntime
          ? t(`settings:desktopConnection.status.${connection.workerRuntimeState?.kind ?? "unknown"}`)
          : t("settings:serverDetail")}
        title={t(desktopRuntime ? "settings:desktopConnection.title" : "settings:serverTitle")}
      />
      {desktopRuntime ? (
        <DesktopConnectionDetails connection={connection} />
      ) : (
        <form
          className="mt-6 space-y-5"
          onSubmit={(event) => {
            event.preventDefault();
            if (!hasChanges) return;
            setConnection(draft);
            void models.refetch();
          }}
        >
          <FieldBlock label={t("settings:fields.authToken")}>
            <Input autoComplete="current-password" readOnly title={t("settings:fields.authManagedByLogin")} type="password" value="" />
          </FieldBlock>
          <div className="flex items-center justify-end gap-3">
            <Button
              onClick={() => {
                setDraft(connection);
                setLocale("en");
              }}
              type="button"
              variant="ghost"
            >
              {t("settings:reset")}
            </Button>
            <Button disabled={!hasChanges} type="submit">
              <CheckCircle2 size={16} />
              {t("common:save")}
            </Button>
          </div>
        </form>
      )}
    </Panel>
  );
}

function DesktopConnectionDetails({ connection }: Pick<SettingsPageState, "connection">) {
  const { t } = useTranslation("settings");
  const { copied, copy } = useCopyText(connection.userId);
  if (connection.authStatus !== "authenticated") return null;

  return (
    <div className="mt-5 space-y-3 border-t border-border/70 pt-4 text-sm">
      <dl className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <dt className="text-muted">{t("desktopConnection.account")}</dt>
        <dd className="min-w-0 break-all font-medium text-ink">{connection.authUsername}</dd>
      </dl>
      <details className="text-muted">
        <summary className="w-fit cursor-pointer">{t("desktopConnection.diagnostics")}</summary>
        <div className="mt-3 flex flex-wrap items-center gap-3">
          <div className="min-w-0">
            <div className="text-xs">{t("desktopConnection.userId")}</div>
            <code className="break-all text-ink">{connection.userId}</code>
          </div>
          <Button onClick={() => { void copy(); }} type="button" variant="ghost">
            {t(copied ? "desktopConnection.copied" : "desktopConnection.copy")}
          </Button>
        </div>
      </details>
    </div>
  );
}
