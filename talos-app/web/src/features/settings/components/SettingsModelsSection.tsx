import { SettingsLlmConfigEditor } from "./models/SettingsLlmConfigEditor";

export function SettingsModelsSection({ nodeId }: { nodeId?: string } = {}) {
  return <SettingsLlmConfigEditor nodeId={nodeId} />;
}
