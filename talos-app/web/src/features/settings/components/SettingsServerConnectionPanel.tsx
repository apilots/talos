import { CheckCircle2, ServerCog } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
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
    <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/88 p-6">
      <SectionHeader
        icon={<ServerCog size={18} />}
        subtitle={t("settings:serverDetail")}
        title={t("settings:serverTitle")}
      />
      {desktopRuntime ? (
        <div className="mt-5 rounded-[var(--radius-control)] border border-border/70 bg-background/55 p-4 text-sm">
          <div className="font-medium text-ink">Desktop local runtime</div>
          <div className="mt-1 text-muted">
            Connected through the local app-server bridge as ordinary user{" "}
            <span className="font-medium text-ink">{connection.userId}</span>. Admin features are not available in desktop mode.
          </div>
        </div>
      ) : null}
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
    </Panel>
  );
}
