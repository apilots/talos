import type { Plugin } from "@talos/sdk-browser";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { mcpServerDefinition, pluginTitle } from "@/features/plugins/lib/pluginDisplay";
import { FormSection, TextInput } from "./McpFormPrimitives";
import { McpServerFormHeader } from "./McpServerFormHeader";
import { HttpTransportFields, StdioTransportFields } from "./McpTransportFields";
import {
  McpTransportSelector,
  type McpTransportMode,
} from "./McpTransportSelector";

type McpServerFormProps =
  | {
      mode: "create";
      onBack: () => void;
    }
  | {
      busy: boolean;
      mode: "update";
      plugin: Plugin;
      onBack: () => void;
      onDisable: (plugin: Plugin) => void;
      onEnable: (plugin: Plugin) => void;
      onUninstall: (plugin: Plugin) => void;
    };

export function McpServerForm(props: McpServerFormProps) {
  const { t } = useTranslation("settings");
  const updatePlugin = props.mode === "update" ? props.plugin : null;
  const definition = updatePlugin ? mcpServerDefinition(updatePlugin) : null;
  const pluginName = updatePlugin ? definition?.display_name ?? definition?.name ?? pluginTitle(updatePlugin) : "";
  const [transportMode, setTransportMode] = useState<McpTransportMode>(
    definition?.transport.type === "streamable_http" ? "http" : "stdio",
  );
  const stdioTransport = definition?.transport.type === "stdio" ? definition.transport : null;
  const httpTransport = definition?.transport.type === "streamable_http" ? definition.transport : null;
  const updateMode = props.mode === "update";

  return (
    <div className="mx-auto max-w-[760px] pb-10">
      <button
        className="mb-8 inline-flex items-center rounded-[var(--radius-control)] text-sm text-muted transition hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
        onClick={props.onBack}
        type="button"
      >
        &lt; {t("mcpServers.back")}
      </button>

      <McpServerFormHeader
        busy={updateMode ? props.busy : false}
        mode={props.mode}
        onDisable={updateMode ? props.onDisable : undefined}
        onEnable={updateMode ? props.onEnable : undefined}
        onUninstall={updateMode ? props.onUninstall : undefined}
        plugin={updatePlugin ?? undefined}
        pluginName={pluginName}
      />

      {updateMode ? (
        <p className="mb-3 text-sm text-muted">{t("mcpServers.switchTypeRequiresUninstall")}</p>
      ) : null}

      <section className="overflow-hidden rounded-[1.05rem] border border-border/80 bg-card/45">
        <FormSection title={t("mcpServers.name")}>
          <TextInput
            defaultValue={updatePlugin ? definition?.name ?? updatePlugin.metadata.name : ""}
            placeholder={t("mcpServers.serverNamePlaceholder")}
            readOnly={updateMode}
          />
        </FormSection>

        <McpTransportSelector
          disabled={updateMode}
          mode={transportMode}
          onChange={setTransportMode}
        />

        {transportMode === "stdio" ? (
          <StdioTransportFields transport={stdioTransport} />
        ) : (
          <HttpTransportFields transport={httpTransport} />
        )}
      </section>

      <div className="mt-4 flex justify-end">
        <button
          className="inline-flex h-10 items-center rounded-[var(--radius-control)] bg-muted/60 px-5 text-sm font-semibold text-card disabled:cursor-not-allowed disabled:opacity-70"
          disabled
          title={t("mcpServers.savePendingApi")}
          type="button"
        >
          {t("mcpServers.save")}
        </button>
      </div>
    </div>
  );
}
