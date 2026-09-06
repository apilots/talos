import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type {
  WebSearchProviderRuntimeConfig,
  WebSearchToolConfig,
} from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import {
  updateWebSearchConfig,
  type WebSearchConfigResponse,
} from "@/features/settings/api";
import {
  useWebSearchConfigQuery,
  webSearchConfigKey,
} from "@/features/settings/hooks/useWebSearchConfigQuery";
import { useConnectionKey } from "@/features/chat";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import {
  SettingsConfigSaveFooter,
  SettingsProviderKeyField,
  SettingsProviderTabs,
} from "../provider-config/SettingsProviderConfigPrimitives";

type WebSearchProviderKey = "firecrawl" | "brave" | "tavily";

const PROVIDERS: Array<{ key: WebSearchProviderKey; label: string }> = [
  { key: "firecrawl", label: "Firecrawl" },
  { key: "brave", label: "Brave" },
  { key: "tavily", label: "Tavily" },
];

function configured(provider: WebSearchProviderRuntimeConfig | undefined): boolean {
  return provider?.api_key === "********";
}

export function SettingsWebSearchConfigEditor() {
  const { t } = useTranslation("settings");
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const [activeProvider, setActiveProvider] = useState<WebSearchProviderKey>("firecrawl");
  const [draft, setDraft] = useState<WebSearchToolConfig | null>(null);
  const { data: serverRead, isLoading, error } = useWebSearchConfigQuery();
  const serverConfig = serverRead?.config;

  useEffect(() => {
    if (serverConfig && !draft) setDraft(serverConfig);
  }, [serverConfig, draft]);

  const mutation = useMutation({
    mutationFn: updateWebSearchConfig,
    onSuccess: (updated: WebSearchConfigResponse) => {
      queryClient.setQueryData(webSearchConfigKey(connectionKey), updated);
      setDraft(updated.config);
    },
  });

  function updateProvider(
    key: WebSearchProviderKey,
    patch: Partial<WebSearchProviderRuntimeConfig>,
  ) {
    if (!draft) return;
    setDraft({ ...draft, [key]: { ...draft[key], ...patch } });
  }

  const isDirty = draft && serverConfig ? JSON.stringify(draft) !== JSON.stringify(serverConfig) : false;

  if (error) {
    return (
      <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
        <div className="text-sm text-danger">{userVisibleErrorMessage(error, "Request failed.")}</div>
      </Panel>
    );
  }

  if (isLoading || !draft || !serverRead) {
    return (
      <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
        <div className="text-sm text-muted">{t("webSearchPanel.loading")}</div>
      </Panel>
    );
  }

  const providerConfig = draft[activeProvider];
  const serverProviderConfig = serverConfig?.[activeProvider];

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
      <div className="mb-5">
        <div className="text-sm font-medium text-ink">{t("webSearchPanel.title")}</div>
        <div className="mt-1 text-sm text-muted">{t("webSearchPanel.detail")}</div>
      </div>

      <div className="mb-5 flex flex-wrap items-center gap-4">
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted">{t("webSearchPanel.activeProvider")}</span>
          <select
            className="rounded-lg border border-border/70 bg-card px-3 py-1.5 text-sm text-ink focus:outline-none"
            onChange={(event) =>
              setDraft({ ...draft, provider: event.target.value as WebSearchProviderKey })}
            value={draft.provider}
          >
            {PROVIDERS.map(({ key, label }) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
          </select>
        </div>
      </div>

      <SettingsProviderTabs
        activeProvider={activeProvider}
        onChange={setActiveProvider}
        providers={PROVIDERS}
      />

      <div className="space-y-3">
        <SettingsProviderKeyField
          clearLabel={t("webSearchPanel.clearApiKey")}
          configured={configured(providerConfig) || configured(serverProviderConfig)}
          label={t("webSearchPanel.apiKey")}
          onChange={(value) => updateProvider(activeProvider, { api_key: value })}
          onClear={() => updateProvider(activeProvider, { api_key: "" })}
          value={providerConfig.api_key ?? ""}
        />

        <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
          <span className="w-24 shrink-0 text-sm text-muted">{t("webSearchPanel.baseUrl")}</span>
          <Input
            aria-label={t("webSearchPanel.baseUrl")}
            className="flex-1 text-sm font-mono"
            onChange={(event) =>
              updateProvider(activeProvider, { base_url: event.target.value || undefined })}
            placeholder="https://api.example.com"
            value={providerConfig.base_url ?? ""}
          />
        </div>

        <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
          <span className="w-24 shrink-0 text-sm text-muted">{t("webSearchPanel.timeout")}</span>
          <Input
            aria-label={t("webSearchPanel.timeout")}
            className="w-40 text-sm font-mono"
            min={1}
            onChange={(event) => {
              const timeout = Number(event.target.value);
              updateProvider(activeProvider, {
                timeout_ms: Number.isFinite(timeout) && timeout > 0 ? timeout : 1,
              });
            }}
            type="number"
            value={providerConfig.timeout_ms}
          />
          <span className="text-sm text-muted">{t("webSearchPanel.milliseconds")}</span>
        </div>
      </div>

      <SettingsConfigSaveFooter
        dirty={isDirty}
        error={mutation.isError ? mutation.error : null}
        onSave={() => draft && mutation.mutate(draft)}
        pending={mutation.isPending}
        saveLabel={t("webSearchPanel.saveChanges")}
        savingLabel={t("webSearchPanel.saving")}
        unsavedLabel={t("webSearchPanel.unsavedChanges")}
      >
        <span className="text-sm text-muted">{t("webSearchPanel.nextTurn")}</span>
      </SettingsConfigSaveFooter>
    </Panel>
  );
}
