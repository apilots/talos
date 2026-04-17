import { CheckCircle2, ServerCog } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { FieldBlock, SectionHeader, ToggleRow } from "./SectionPrimitives";

export function SettingsServerSection({
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
}: Pick<
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
>) {
  const { t } = useTranslation(["settings", "common"]);

  return (
    <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
      <SectionHeader
        icon={<ServerCog size={18} />}
        subtitle={t("settings:serverDetail")}
        title={t("settings:serverTitle")}
      />
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
          <FieldBlock label={t("settings:fields.userId")}>
            <Input
              onChange={(event) => setDraft((current) => ({ ...current, userId: event.target.value }))}
              value={draft.userId}
            />
          </FieldBlock>
        </div>
        <FieldBlock label={t("settings:fields.authToken")}>
          <Input
            autoComplete="current-password"
            onChange={(event) => setDraft((current) => ({ ...current, authToken: event.target.value }))}
            type="password"
            value={draft.authToken}
          />
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
