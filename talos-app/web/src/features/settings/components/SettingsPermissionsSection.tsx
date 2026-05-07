import { ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { Panel } from "@/shared/ui/Panel";
import { SectionHeader, ToggleRow } from "./SectionPrimitives";

export function SettingsPermissionsSection({
  safeFileReads,
  safeFileWrites,
  terminalApproval,
  setSafeFileReads,
  setSafeFileWrites,
  setTerminalApproval,
}: Pick<
  SettingsPageState,
  | "safeFileReads"
  | "safeFileWrites"
  | "terminalApproval"
  | "setSafeFileReads"
  | "setSafeFileWrites"
  | "setTerminalApproval"
>) {
  const { t } = useTranslation(["settings"]);

  return (
    <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/88 p-6">
      <SectionHeader
        icon={<ShieldCheck size={18} />}
        subtitle={t("settings:permissionsDetail")}
        title={t("settings:permissionsTitle")}
      />
      <div className="mt-6 grid gap-4 lg:grid-cols-3">
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
    </Panel>
  );
}
