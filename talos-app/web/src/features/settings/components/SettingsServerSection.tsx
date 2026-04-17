import type { SettingsPageState } from "../hooks/useSettingsPage";
import { SettingsServerConnectionPanel } from "./SettingsServerConnectionPanel";
import { SettingsWorkspaceDocsPanel } from "./SettingsWorkspaceDocsPanel";

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
  return (
    <div className="space-y-5">
      <SettingsServerConnectionPanel
        connection={connection}
        draft={draft}
        models={models}
        safeFileReads={safeFileReads}
        safeFileWrites={safeFileWrites}
        setConnection={setConnection}
        setDraft={setDraft}
        setLocale={setLocale}
        setSafeFileReads={setSafeFileReads}
        setSafeFileWrites={setSafeFileWrites}
        setTerminalApproval={setTerminalApproval}
        terminalApproval={terminalApproval}
      />
      <SettingsWorkspaceDocsPanel />
    </div>
  );
}
