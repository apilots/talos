import type { SettingsPageState } from "../hooks/useSettingsPage";
import { SettingsServerConnectionPanel } from "./SettingsServerConnectionPanel";

export function SettingsServerSection({
  connection,
  setConnection,
  draft,
  setDraft,
  models,
  setLocale,
}: Pick<
  SettingsPageState,
  | "connection"
  | "setConnection"
  | "draft"
  | "setDraft"
  | "models"
  | "setLocale"
>) {
  return (
    <SettingsServerConnectionPanel
      connection={connection}
      draft={draft}
      models={models}
      setConnection={setConnection}
      setDraft={setDraft}
      setLocale={setLocale}
    />
  );
}
