import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { CustomProviderConfig, LlmConfig, ProviderConfig } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import { updateLlmConfig, type LlmConfigResponse } from "@/features/settings/api";
import { llmConfigKey, useLlmConfigQuery } from "@/features/settings/hooks/useLlmConfigQuery";
import { chatKeys } from "@/features/chat";
import { useEnvironmentKey } from "@/features/computers";
import { Panel } from "@/shared/ui/Panel";
import { createUuid } from "@/shared/lib/uuid";
import { CustomProviderMenu } from "./CustomProviderMenu";
import { CustomProviderModal } from "./CustomProviderModal";
import { CUSTOM_PROTOCOLS } from "./providerProtocol";

import { CustomProviderHeader } from "./CustomProviderHeader";
import { SettingsModelDeleteDialog } from "./SettingsModelDeleteDialog";
import { SettingsProviderModels } from "./SettingsProviderModels";
import {
  SettingsConfigSaveFooter,
  SettingsProviderTabs,
} from "../provider-config/SettingsProviderConfigPrimitives";

type ProviderKey = Exclude<keyof LlmConfig, "provider" | "model" | "custom_providers">;

const PROVIDER_LABELS = {
  anthropic: "Anthropic",
  openai: "OpenAI",
  bailian: "Bailian",
  deepseek: "DeepSeek",
  ollama: "Ollama",
  openrouter: "OpenRouter",
} satisfies Record<ProviderKey, string>;

function providerConfig(llm: LlmConfig, id: string): ProviderConfig | undefined {
  return Object.hasOwn(PROVIDER_LABELS, id) ? llm[id as ProviderKey] : llm.custom_providers?.[id];
}

const PROVIDERS = Object.entries(PROVIDER_LABELS).map(([key, label]) => ({
  key: key as ProviderKey,
  label,
}));

// ─── Main section ─────────────────────────────────────────────────────────────

export function SettingsLlmConfigEditor() {
  const { t } = useTranslation("settings");
  const connectionKey = useEnvironmentKey();
  const queryClient = useQueryClient();
  const [activeProvider, setActiveProvider] = useState<string>("anthropic");
  const [draft, setDraft] = useState<LlmConfig | null>(null);
  const [providerModal, setProviderModal] = useState<"add" | "edit" | null>(null);
  const [providerToDelete, setProviderToDelete] = useState<{ id: string; name: string } | null>(null);
  const customTriggerRef = useRef<HTMLButtonElement>(null);

  const { data: serverRead, isLoading, error } = useLlmConfigQuery();
  const serverConfig = serverRead?.config;

  // Keep draft in sync with server on initial load (but not on mutation success)
  useEffect(() => {
    if (serverConfig && !draft) setDraft(serverConfig);
  }, [serverConfig, draft]);

  const mutation = useMutation({
    mutationFn: updateLlmConfig,
    onSuccess: (updated: LlmConfigResponse) => {
      queryClient.setQueryData(llmConfigKey(connectionKey), updated);
      setDraft(updated.config);
      void queryClient.invalidateQueries({ queryKey: chatKeys.models(connectionKey) });
    },
  });

  function updateProvider(key: string, patch: Partial<ProviderConfig>) {
    if (!draft) return;
    if (Object.hasOwn(PROVIDER_LABELS, key)) {
      setDraft({ ...draft, [key]: { ...draft[key as ProviderKey], ...patch } });
    } else {
      const existing = draft.custom_providers?.[key];
      if (existing) setDraft({ ...draft, custom_providers: { ...draft.custom_providers, [key]: { ...existing, ...patch } } });
    }
  }

  function setDefaultProvider(key: string) {
    if (!draft) return;
    const models = providerConfig(draft, key)?.models ?? [];
    setDraft({ ...draft, provider: key, model: models.some((model) => model.id === draft.model) ? draft.model : models[0]?.id ?? "" });
  }

  function saveCustomProvider(provider: CustomProviderConfig) {
    if (!draft) return;
    const id = providerModal === "edit" ? activeProvider : `custom-${createUuid()}`;
    setDraft({ ...draft, custom_providers: { ...draft.custom_providers, [id]: provider } });
    setActiveProvider(id);
    setProviderModal(null);
  }

  function deleteCustomProvider() {
    if (!draft || !providerToDelete || providerToDelete.id === draft.provider) return;
    const providers = { ...draft.custom_providers };
    delete providers[providerToDelete.id];
    setDraft({ ...draft, custom_providers: providers });
    setActiveProvider("anthropic");
    setProviderToDelete(null);
  }

  function setDefaultModel(modelId: string) {
    if (!draft) return;
    setDraft({ ...draft, model: modelId });
  }

  const isDirty = draft && serverConfig ? JSON.stringify(draft) !== JSON.stringify(serverConfig) : false;

  if (error) {
    return (
      <Panel className="rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] p-5">
        <div className="text-sm text-danger">{userVisibleErrorMessage(error, "Request failed.")}</div>
      </Panel>
    );
  }

  if (isLoading || !draft || !serverRead) {
    return (
      <Panel className="rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] p-5">
        <div className="text-sm text-muted">{t("modelsPanel.loading")}</div>
      </Panel>
    );
  }

  const customProvider = draft.custom_providers?.[activeProvider];
  const selectedConfig = providerConfig(draft, activeProvider);
  const defaultModels = providerConfig(draft, draft.provider)?.models ?? [];
  const providerOptions = [...PROVIDERS, ...Object.entries(draft.custom_providers ?? {}).flatMap(([key, entry]) => entry ? [{ key, label: entry.name }] : [])];
  const activeProviderCapabilities = customProvider ? { default_protocol: customProvider.protocol ?? "chat", protocols: CUSTOM_PROTOCOLS } : serverRead.provider_capabilities[activeProvider];
  if (!activeProviderCapabilities || !selectedConfig) {
    return (
      <Panel className="rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] p-5">
        <div className="text-sm text-danger">
          {t("modelsPanel.providerCapabilitiesMissing", { provider: activeProvider })}
        </div>
      </Panel>
    );
  }

  return (
    <Panel className="rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] p-5">
      {/* Title */}
      <div className="mb-5">
        <div className="text-sm font-medium text-ink">{t("modelsPanel.title")}</div>
        <div className="mt-1 text-sm text-muted">{t("modelsPanel.detail")}</div>
      </div>

      {/* Top controls: Active Provider + Default Model */}
      <div className="mb-5 flex flex-wrap items-center gap-4">
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted">{t("modelsPanel.activeProvider")}</span>
          <select
            value={draft.provider}
            onChange={(e) => setDefaultProvider(e.target.value)}
            className="rounded-lg border border-border/70 bg-card px-3 py-1.5 text-sm text-ink focus:outline-none"
          >
            {providerOptions.map(({ key, label }) => (
              <option key={key} value={key}>{label}</option>
            ))}
          </select>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted">{t("modelsPanel.defaultModel")}</span>
          <select
            value={draft.model}
            onChange={(e) => setDefaultModel(e.target.value)}
            className="rounded-lg border border-border/70 bg-card px-3 py-1.5 text-sm text-ink focus:outline-none"
          >
            {defaultModels.map((m) => (
              <option key={m.id} value={m.id}>{m.name || m.id}</option>
            ))}
            {defaultModels.length === 0 && (
              <option value={draft.model}>{draft.model}</option>
            )}
          </select>
        </div>
      </div>

      <SettingsProviderTabs
        activeProvider={activeProvider}
        onChange={setActiveProvider}
        providers={PROVIDERS}
        trailing={<CustomProviderMenu providers={draft.custom_providers ?? {}} selected={activeProvider} onSelect={setActiveProvider} onAdd={() => setProviderModal("add")} triggerRef={customTriggerRef} />}
      />

      {customProvider && (
        <CustomProviderHeader
          deleteDisabled={activeProvider === draft.provider}
          name={customProvider.name}
          onDelete={() => setProviderToDelete({ id: activeProvider, name: customProvider.name })}
          onEdit={() => setProviderModal("edit")}
        />
      )}
      {providerToDelete && (
        <SettingsModelDeleteDialog
          fallbackFocusRef={customTriggerRef}
          kind="provider"
          name={providerToDelete.name}
          onClose={() => setProviderToDelete(null)}
          onConfirm={deleteCustomProvider}
        />
      )}
      {providerModal && <CustomProviderModal initial={providerModal === "edit" ? customProvider : undefined} onClose={() => setProviderModal(null)} onSave={saveCustomProvider} />}
      {/* Active provider panel — key resets local modal/edit state on tab switch */}
      <SettingsProviderModels
        key={activeProvider}
        config={selectedConfig}
        custom={Boolean(customProvider)}
        capabilities={activeProviderCapabilities}
        keyConfigured={serverRead?.provider_keys[activeProvider]?.api_key_configured ?? false}
        onChange={(patch) => updateProvider(activeProvider, patch)}
      />

      <SettingsConfigSaveFooter
        dirty={isDirty}
        error={mutation.isError ? mutation.error : null}
        onSave={() => draft && mutation.mutate(draft)}
        pending={mutation.isPending}
        saveLabel={t("modelsPanel.saveChanges")}
        savingLabel={t("modelsPanel.saving")}
        unsavedLabel={t("modelsPanel.unsavedChanges")}
      />
    </Panel>
  );
}
