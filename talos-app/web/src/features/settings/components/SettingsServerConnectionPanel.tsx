import { CheckCircle2, ServerCog } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { isDesktopRuntime } from "@/shared/api/desktop-client";
import { FieldBlock, SectionHeader, ToggleRow } from "./SectionPrimitives";

export type SettingsServerConnectionProps = Pick<
  SettingsPageState,
  | "connection"
  | "setConnection"
  | "draft"
  | "setDraft"
  | "models"
  | "setLocale"
  | "setSafeFileReads"
  | "setSafeFileWrites"
  | "setTerminalApproval"
  | "safeFileReads"
  | "safeFileWrites"
  | "terminalApproval"
>;

export function SettingsServerConnectionPanel({
  connection,
  setConnection,
  draft,
  setDraft,
  models,
  setLocale,
  setSafeFileReads,
  setSafeFileWrites,
  setTerminalApproval,
  safeFileReads,
  safeFileWrites,
  terminalApproval,
}: SettingsServerConnectionProps) {
  const { t } = useTranslation(["settings", "common"]);
  const desktopRuntime = isDesktopRuntime();

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
          setConnection(draft);
          void models.refetch();
        }}
      >
        <div className="grid gap-4 md:grid-cols-2">
          <FieldBlock label={t("settings:fields.serverUrl")}>
            <Input
              onChange={(event) => setDraft((current) => ({ ...current, serverUrl: event.target.value }))}
              value={draft.serverUrl}
            />
          </FieldBlock>
        </div>
        <FieldBlock label={t("settings:fields.authToken")}>
          <Input autoComplete="current-password" readOnly title={t("settings:fields.authManagedByLogin")} type="password" value="" />
        </FieldBlock>
        <div className="grid gap-4 md:grid-cols-3">
          <ToggleRow
            description={t("settings:fields.fileReadsDetail")}
            label={t("settings:fields.fileReads")}
            onToggle={() => setSafeFileReads((current) => !current)}
            value={safeFileReads}
          />
          <ToggleRow
            description={t("settings:fields.fileWritesDetail")}
            label={t("settings:fields.fileWrites")}
            onToggle={() => setSafeFileWrites((current) => !current)}
            value={safeFileWrites}
          />
          <ToggleRow
            description={t("settings:fields.terminalCommandsDetail")}
            label={t("settings:fields.terminalCommands")}
            onToggle={() => setTerminalApproval((current) => !current)}
            value={terminalApproval}
          />
        </div>
        <FieldBlock label={t("settings:detectedWorkspace")}>
          <Input disabled value={__TALOS_DEFAULT_PROJECT_ROOT__} />
        </FieldBlock>
        <div className="flex items-center justify-end gap-3">
          <Button
            onClick={() => {
              setDraft(connection);
              setLocale("en");
              setSafeFileReads(true);
              setSafeFileWrites(false);
              setTerminalApproval(true);
            }}
            type="button"
            variant="ghost"
          >
            {t("settings:reset")}
          </Button>
          <Button type="submit">
            <CheckCircle2 size={16} />
            {t("common:save")}
          </Button>
        </div>
      </form>
    </Panel>
  );
}
