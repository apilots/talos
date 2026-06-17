import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type {
  LlmConfig,
  LlmConfigRead,
  LlmProtocol,
  ProviderConfig,
} from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";
import { Brain, Pencil, Plus, Trash2 } from "lucide-react";

import { getLlmConfig, updateLlmConfig } from "@/features/settings/api";
import { useConnectionKey } from "@/features/chat";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { SettingsModelModal } from "./SettingsModelModal";
import {
  INPUT_KIND_ICONS,
  INPUT_KINDS,
  emptyModelForm,
  formToModel,
  modelToForm,
  type ModelFormState,
} from "./modelForm";

type ProviderKey = "anthropic" | "openai" | "bailian" | "ollama" | "openrouter";

const PROVIDERS: { key: ProviderKey; label: string }[] = [
  { key: "anthropic", label: "Anthropic" },
  { key: "openai", label: "OpenAI" },
  { key: "bailian", label: "Bailian" },
  { key: "ollama", label: "Ollama" },
  { key: "openrouter", label: "OpenRouter" },
];

const PROVIDER_PROTOCOLS: Record<ProviderKey, LlmProtocol[]> = {
  anthropic: ["messages"],
  openai: ["chat", "responses"],
  bailian: ["chat"],
  ollama: ["chat"],
  openrouter: ["chat"],
};

const DEFAULT_PROTOCOLS: Record<ProviderKey, LlmProtocol> = {
  anthropic: "messages",
  openai: "chat",
  bailian: "chat",
  ollama: "chat",
  openrouter: "chat",
};

function fmtContext(n: number): string {
  if (n >= 1000) return `${Math.round(n / 1000)}K`;
  return String(n);
}

function parseErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return "Request failed.";
}

function protocolLabel(protocol: LlmProtocol, t: (key: string) => string): string {
  switch (protocol) {
    case "chat":
      return t("modelsPanel.protocolChat");
    case "responses":
      return t("modelsPanel.protocolResponses");
    case "messages":
      return t("modelsPanel.protocolMessages");
  }
}

// ─── Provider tab content ─────────────────────────────────────────────────────

interface ProviderTabProps {
  providerKey: ProviderKey;
  config: ProviderConfig;
  keyConfigured: boolean;
  onChange: (patch: Partial<ProviderConfig>) => void;
}

function ProviderTab({ providerKey, config, keyConfigured, onChange }: ProviderTabProps) {
  const { t } = useTranslation("settings");
  const [modalMode, setModalMode] = useState<"add" | "edit" | null>(null);
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [modelForm, setModelForm] = useState<ModelFormState>(emptyModelForm);
  const [modelFormError, setModelFormError] = useState("");
  const protocols = PROVIDER_PROTOCOLS[providerKey];
  const protocol = config.protocol ?? DEFAULT_PROTOCOLS[providerKey];

  function openAddModal() { setModalMode("add"); setEditingIdx(null); setModelForm(emptyModelForm); setModelFormError(""); }
  function openEditModal(idx: number) { setModalMode("edit"); setEditingIdx(idx); setModelForm(modelToForm(config.models[idx])); setModelFormError(""); }
  function closeModal() { setModalMode(null); setEditingIdx(null); setModelFormError(""); }

  function saveModel() {
    if (!modelForm.id.trim()) { setModelFormError(t("modelsPanel.modelIdRequired")); return; }
    const models = [...config.models];
    const updated = formToModel(modelForm, editingIdx != null ? config.models[editingIdx] : undefined);
    if (editingIdx == null && models.some((m) => m.id === updated.id)) { setModelFormError(t("modelsPanel.modelIdDuplicate")); return; }
    if (editingIdx != null) { models[editingIdx] = updated; } else { models.push(updated); }
    onChange({ models });
    closeModal();
  }

  function deleteModel(idx: number) {
    if (!window.confirm(t("modelsPanel.deleteConfirm"))) return;
    onChange({ models: config.models.filter((_, i) => i !== idx) });
  }

  return (
    <>
      {/* Enabled (status) + API Key + Base URL */}
      <div className="mb-5 space-y-3">
        {/* Enabled — read-only status, controlled by Active Provider selection */}
        <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
          <span className="w-24 text-sm text-muted">{t("modelsPanel.enabled")}</span>
          <span className={["text-sm font-semibold", config.enabled ? "text-accent" : "text-muted"].join(" ")}>
            {config.enabled ? t("modelsPanel.on") : t("modelsPanel.off")}
          </span>
        </div>

        <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
          <span className="w-24 text-sm text-muted">{t("modelsPanel.protocol")}</span>
          {protocols.length > 1 ? (
            <select
              value={protocol}
              onChange={(e) => onChange({ protocol: e.target.value as LlmProtocol })}
              className="rounded-lg border border-border/70 bg-card px-3 py-1.5 text-sm text-ink focus:outline-none"
            >
              {protocols.map((option) => (
                <option key={option} value={option}>
                  {protocolLabel(option, t)}
                </option>
              ))}
            </select>
          ) : (
            <span className="text-sm font-medium text-ink">{protocolLabel(protocol, t)}</span>
          )}
        </div>

        {/* API Key */}
        <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
          <span className="w-24 shrink-0 text-sm text-muted">{t("modelsPanel.apiKey")}</span>
          <Input
            type="password"
            value={config.api_key ?? ""}
            onChange={(e) => onChange({ api_key: e.target.value })}
            className="flex-1 text-sm font-mono"
          />
          {keyConfigured && (
            <Button variant="ghost" onClick={() => onChange({ api_key: "" })} type="button">
              {t("modelsPanel.clearApiKey")}
            </Button>
          )}
        </div>

        {/* Base URL */}
        <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
          <span className="w-24 shrink-0 text-sm text-muted">{t("modelsPanel.baseUrl")}</span>
          <Input
            value={config.base_url ?? ""}
            onChange={(e) => onChange({ base_url: e.target.value || null })}
            placeholder="https://api.openai.com/v1"
            className="flex-1 text-sm font-mono"
          />
        </div>
      </div>

      {/* Models table */}
      <div className="mb-3 flex items-center justify-between">
        <div className="text-sm font-medium text-ink">{t("modelsPanel.modelsList")}</div>
        <Button variant="secondary" onClick={openAddModal} type="button">
          <Plus size={14} className="mr-1.5" />
          {t("modelsPanel.add")}
        </Button>
      </div>

      {config.models.length === 0 ? (
        <div className="rounded-[1.2rem] border border-dashed border-border/80 px-4 py-8 text-center text-sm text-muted">
          {t("modelsPanel.noModels")}
        </div>
      ) : (
        <div className="space-y-2">
          <div className="grid grid-cols-[minmax(0,1.8fr)_minmax(0,1.4fr)_80px_60px_80px] gap-3 px-4 text-xs uppercase tracking-[0.14em] text-muted">
            <div>{t("modelsPanel.colId")}</div>
            <div>{t("modelsPanel.colName")}</div>
            <div>{t("modelsPanel.colContext")}</div>
            <div>{t("modelsPanel.colCaps")}</div>
            <div className="text-right">{t("modelsPanel.colActions")}</div>
          </div>
          {config.models.map((model, idx) => (
            <div key={model.id} className="grid grid-cols-[minmax(0,1.8fr)_minmax(0,1.4fr)_80px_60px_80px] items-center gap-3 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-3">
              <div className="min-w-0 truncate font-mono text-sm font-medium text-ink">{model.id}</div>
              <div className="truncate text-sm text-muted">{model.name || "—"}</div>
              <div className="text-sm text-muted">{fmtContext(model.context_window)}</div>
              <div className="flex items-center gap-1">
                {model.reasoning && <Brain size={13} className="text-accent" />}
                {INPUT_KINDS.filter((k) => model.input.includes(k)).map((k) => {
                  const KindIcon = INPUT_KIND_ICONS[k];
                  return <KindIcon key={k} size={13} className="text-muted" />;
                })}
              </div>
              <div className="flex justify-end">
                <div className="inline-flex items-center gap-0.5 rounded-[999px] border border-border/75 bg-surface/70 p-1">
                  <button className="inline-flex h-7 w-7 items-center justify-center rounded-full text-muted transition hover:bg-background hover:text-ink" onClick={() => openEditModal(idx)} type="button">
                    <Pencil size={13} />
                  </button>
                  <button className="inline-flex h-7 w-7 items-center justify-center rounded-full text-muted transition hover:bg-danger/10 hover:text-danger" onClick={() => deleteModel(idx)} type="button">
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {modalMode && (
        <SettingsModelModal
          mode={modalMode}
          form={modelForm}
          saving={false}
          error={modelFormError}
          onChange={(f) => { setModelForm(f); setModelFormError(""); }}
          onSave={saveModel}
          onClose={closeModal}
        />
      )}
    </>
  );
}

// ─── Main section ─────────────────────────────────────────────────────────────

export function SettingsModelsEditor() {
  const { t } = useTranslation("settings");
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const [activeProvider, setActiveProvider] = useState<ProviderKey>("anthropic");
  const [draft, setDraft] = useState<LlmConfig | null>(null);

  const { data: serverRead, isLoading, error } = useQuery({
    queryKey: ["llm-config", connectionKey],
    queryFn: getLlmConfig,
  });
  const serverConfig = serverRead?.config;

  // Keep draft in sync with server on initial load (but not on mutation success)
  useEffect(() => {
    if (serverConfig && !draft) setDraft(serverConfig);
  }, [serverConfig, draft]);

  const mutation = useMutation({
    mutationFn: updateLlmConfig,
    onSuccess: (updated: LlmConfigRead) => {
      queryClient.setQueryData(["llm-config", connectionKey], updated);
      setDraft(updated.config);
    },
  });

  function updateProvider(key: ProviderKey, patch: Partial<ProviderConfig>) {
    if (!draft) return;
    setDraft({ ...draft, [key]: { ...draft[key], ...patch } });
  }

  function setDefaultProvider(key: ProviderKey) {
    if (!draft) return;
    // selecting active provider implies enabled=true for that provider
    setDraft({ ...draft, provider: key, [key]: { ...draft[key], enabled: true } });
  }

  function setDefaultModel(modelId: string) {
    if (!draft) return;
    setDraft({ ...draft, model: modelId });
  }

  const isDirty = draft && serverConfig && JSON.stringify(draft) !== JSON.stringify(serverConfig);

  if (error) {
    return (
      <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
        <div className="text-sm text-danger">{parseErrorMessage(error)}</div>
      </Panel>
    );
  }

  if (isLoading || !draft) {
    return (
      <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
        <div className="text-sm text-muted">{t("modelsPanel.loading")}</div>
      </Panel>
    );
  }

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
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
            onChange={(e) => setDefaultProvider(e.target.value as ProviderKey)}
            className="rounded-lg border border-border/70 bg-card px-3 py-1.5 text-sm text-ink focus:outline-none"
          >
            {PROVIDERS.map(({ key, label }) => (
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
            {draft[draft.provider as ProviderKey].models.map((m) => (
              <option key={m.id} value={m.id}>{m.name || m.id}</option>
            ))}
            {draft[draft.provider as ProviderKey].models.length === 0 && (
              <option value={draft.model}>{draft.model}</option>
            )}
          </select>
        </div>
      </div>

      {/* Provider tabs */}
      <div className="mb-5 flex gap-1 rounded-[1rem] border border-border/70 bg-background/40 p-1">
        {PROVIDERS.map(({ key, label }) => {
          const isActive = activeProvider === key;
          const isEnabled = draft[key].enabled;
          return (
            <button
              key={key}
              type="button"
              onClick={() => setActiveProvider(key)}
              className={[
                "flex flex-1 items-center justify-center gap-1.5 rounded-[0.75rem] px-3 py-2 text-sm font-medium transition",
                isActive ? "bg-card shadow-sm text-ink" : "text-muted hover:text-ink",
              ].join(" ")}
            >
              {label}
              {isEnabled && <span className="h-1.5 w-1.5 rounded-full bg-accent" />}
            </button>
          );
        })}
      </div>

      {/* Active provider panel — key resets local modal/edit state on tab switch */}
      <ProviderTab
        key={activeProvider}
        providerKey={activeProvider}
        config={draft[activeProvider]}
        keyConfigured={serverRead?.provider_keys[activeProvider]?.api_key_configured ?? false}
        onChange={(patch) => updateProvider(activeProvider, patch)}
      />

      {/* Footer: Save Changes */}
      <div className="mt-6 flex items-center justify-between border-t border-border/60 pt-4">
        {mutation.isError && (
          <span className="text-sm text-danger">{parseErrorMessage(mutation.error)}</span>
        )}
        {!mutation.isError && isDirty && (
          <span className="text-sm text-muted">{t("modelsPanel.unsavedChanges")}</span>
        )}
        {!mutation.isError && !isDirty && <span />}
        <Button
          onClick={() => draft && mutation.mutate(draft)}
          disabled={mutation.isPending || !isDirty}
          type="button"
        >
          {mutation.isPending ? t("modelsPanel.saving") : t("modelsPanel.saveChanges")}
        </Button>
      </div>
    </Panel>
  );
}
