import type { McpTransportDefinition } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import {
  DisabledAddButton,
  firstEntry,
  FormSection,
  InlineInputRow,
  SingleInputRow,
  TextInput,
  TrashIconButton,
} from "./McpFormPrimitives";

type StdioTransport = Extract<McpTransportDefinition, { type: "stdio" }>;
type HttpTransport = Extract<McpTransportDefinition, { type: "streamable_http" }>;

export function StdioTransportFields({ transport }: { transport: StdioTransport | null }) {
  const { t } = useTranslation("settings");
  const envEntry = firstEntry(transport?.env);

  return (
    <>
      <FormSection title={t("mcpServers.command")}>
        <TextInput defaultValue={transport?.command ?? ""} placeholder={t("mcpServers.commandPlaceholder")} />
      </FormSection>
      <FormSection title={t("mcpServers.arguments")}>
        <SingleInputRow>
          <TextInput defaultValue={transport?.args.join(" ") ?? ""} placeholder={t("mcpServers.argumentPlaceholder")} />
          <TrashIconButton />
        </SingleInputRow>
        <DisabledAddButton label={t("mcpServers.addArgument")} />
      </FormSection>
      <FormSection title={t("mcpServers.environmentVariables")}>
        <InlineInputRow>
          <TextInput defaultValue={envEntry?.[0] ?? ""} placeholder={t("mcpServers.key")} />
          <TextInput defaultValue={envEntry?.[1] ?? ""} placeholder={t("mcpServers.value")} />
          <TrashIconButton />
        </InlineInputRow>
        <DisabledAddButton label={t("mcpServers.addEnvironmentVariable")} />
      </FormSection>
      <FormSection title={t("mcpServers.environmentVariablePassthrough")}>
        <SingleInputRow>
          <TextInput placeholder={t("mcpServers.environmentVariablePassthrough")} />
          <TrashIconButton />
        </SingleInputRow>
        <DisabledAddButton label={t("mcpServers.addVariable")} />
      </FormSection>
      <FormSection title={t("mcpServers.workingDirectory")}>
        <TextInput defaultValue={transport?.cwd ?? ""} placeholder="~/code" />
      </FormSection>
    </>
  );
}

export function HttpTransportFields({ transport }: { transport: HttpTransport | null }) {
  const { t } = useTranslation("settings");
  const headerEntry = firstEntry(transport?.headers);

  return (
    <>
      <FormSection title={t("mcpServers.url")}>
        <TextInput defaultValue={transport?.url ?? ""} placeholder="https://mcp.example.com/mcp" />
      </FormSection>
      <FormSection title={t("mcpServers.bearerTokenEnvVar")}>
        <TextInput placeholder={t("mcpServers.bearerTokenPlaceholder")} />
      </FormSection>
      <FormSection title={t("mcpServers.headers")}>
        <InlineInputRow>
          <TextInput defaultValue={headerEntry?.[0] ?? ""} placeholder={t("mcpServers.key")} />
          <TextInput defaultValue={headerEntry?.[1] ?? ""} placeholder={t("mcpServers.value")} />
          <TrashIconButton />
        </InlineInputRow>
        <DisabledAddButton label={t("mcpServers.addHeader")} />
      </FormSection>
      <FormSection title={t("mcpServers.headersFromEnvironmentVariables")}>
        <InlineInputRow>
          <TextInput placeholder={t("mcpServers.key")} />
          <TextInput placeholder={t("mcpServers.value")} />
          <TrashIconButton />
        </InlineInputRow>
        <DisabledAddButton label={t("mcpServers.addVariable")} />
      </FormSection>
    </>
  );
}
