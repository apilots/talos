import { useTranslation } from "react-i18next";
import { FormSection, TextArea, TextInput } from "./McpFormPrimitives";

export type TransportFields = { command: string; args: string; env: string; cwd: string; url: string; headers: string };
export function McpTransportFields({ mode, value, onChange, readOnly }: { mode: "stdio" | "http"; value: TransportFields; onChange: (next: TransportFields) => void; readOnly: boolean }) {
  const { t } = useTranslation("settings");
  const fields: (keyof TransportFields)[] = mode === "stdio" ? ["command", "args", "env", "cwd"] : ["url", "headers"];
  const labels = { command: t("mcpServers.command"), args: t("mcpServers.arguments"), env: t("mcpServers.environmentVariables"), cwd: t("mcpServers.workingDirectory"), url: t("mcpServers.url"), headers: t("mcpServers.headers") };
  return fields.map((field) => <FormSection key={field} title={labels[field]}>
    {["args", "env", "headers"].includes(field)
      ? <TextArea aria-label={labels[field]} onChange={(event) => onChange({ ...value, [field]: event.target.value })} readOnly={readOnly} value={value[field]} />
      : <TextInput aria-label={labels[field]} onChange={(event) => onChange({ ...value, [field]: event.target.value })} readOnly={readOnly} required={field === "command" || field === "url"} value={value[field]} />}
    {["env", "headers"].includes(field) ? <p className="text-xs text-muted">{t("mcpServers.entriesHint")}</p> : field === "args" ? <p className="text-xs text-muted">{t("mcpServers.argumentsHint")}</p> : null}
  </FormSection>);
}
