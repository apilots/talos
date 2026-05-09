import React, { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { LlmConfig, LlmConfigRead, ModelInputKind, ProviderConfig, ProviderModelConfig } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";
import { Brain, File, Image, Mic, Pencil, Plus, Trash2, Type, Video, X } from "lucide-react";

import { getLlmConfig, updateLlmConfig } from "@/features/settings/api";
import { useConnectionKey } from "@/features/chat";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

type ProviderKey = "anthropic" | "openai" | "bailian" | "ollama";

const PROVIDERS: { key: ProviderKey; label: string }[] = [
  { key: "anthropic", label: "Anthropic" },
  { key: "openai", label: "OpenAI" },
  { key: "bailian", label: "Bailian" },
  { key: "ollama", label: "Ollama" },
];

const INPUT_KINDS: ModelInputKind[] = ["text", "image", "audio", "video", "file"];

const INPUT_KIND_ICONS: Record<ModelInputKind, React.ElementType> = {
  text: Type,
  image: Image,
  audio: Mic,
  video: Video,
  file: File,
};

interface ModelFormState {
  id: string;
  name: string;
  context_window: string;
  reasoning: boolean;
  input: ModelInputKind[];
}

const emptyModelForm: ModelFormState = {
  id: "", name: "", context_window: "200000",
  reasoning: false, input: ["text"],
};

function modelToForm(m: ProviderModelConfig): ModelFormState {
  return {
    id: m.id, name: m.name,
    context_window: String(m.context_window),
    reasoning: m.reasoning, input: m.input,
  };
}

function formToModel(f: ModelFormState, existing?: ProviderModelConfig): ProviderModelConfig {
  const base: ProviderModelConfig = existing ?? {
    id: f.id, name: f.name, context_window: 0,
    reasoning: false, input: ["text"],
    cost: { input: 0, output: 0, cache_read: 0, cache_write: 0 },
    compat: {},
  };
  return {
    ...base,
    id: f.id.trim(), name: f.name.trim(),
    context_window: parseInt(f.context_window, 10) || 0,
    reasoning: f.reasoning,
    input: f.input.length > 0 ? f.input : ["text"],
  };
}

function fmtContext(n: number): string {
  if (n >= 1000) return `${Math.round(n / 1000)}K`;
  return String(n);
}

function parseErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return "Request failed.";
}

// ─── Model add/edit modal ─────────────────────────────────────────────────────

interface ModelModalProps {
  mode: "add" | "edit";
  form: ModelFormState;
  saving: boolean;
  error: string;
  onChange: (f: ModelFormState) => void;
  onSave: () => void;
  onClose: () => void;
}

function ModelModal({ mode, form, saving, error, onChange, onSave, onClose }: ModelModalProps) {
  const { t } = useTranslation("admin");
  const isAdd = mode === "add";
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-[rgba(28,22,16,0.46)] px-6 py-10">
      <div className="absolute inset-0" onClick={onClose} />
      <Panel className="relative z-10 w-full max-w-[520px] rounded-[1.5rem] border-border/75 bg-card p-6 shadow-[0_24px_80px_rgba(69,55,34,0.18)]">
        <div className="flex items-start justify-between gap-6">
          <div>
            <div className="text-lg font-semibold text-ink">
              {isAdd ? t("modelsPanel.addModelTitle") : t("modelsPanel.editModelTitle")}
            </div>
            <div className="mt-1 text-sm text-muted">
              {isAdd ? t("modelsPanel.addModelDetail") : t("modelsPanel.editModelDetail")}
            </div>
          </div>
          <button className="inline-flex h-10 w-10 items-center justify-center rounded-full text-muted transition hover:bg-surface hover:text-ink" onClick={onClose} type="button">
            <X size={18} />
          </button>
        </div>
        <div className="mt-5 grid grid-cols-2 gap-4">
          <div className="col-span-2">
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.modelId")}</label>
            <Input value={form.id} onChange={(e) => onChange({ ...form, id: e.target.value })} placeholder="claude-3-5-sonnet" disabled={!isAdd} />
          </div>
          <div className="col-span-2">
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.modelName")}</label>
            <Input value={form.name} onChange={(e) => onChange({ ...form, name: e.target.value })} placeholder="Claude 3.5 Sonnet" />
          </div>
          <div className="col-span-2">
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.contextWindow")}</label>
            <Input value={form.context_window} onChange={(e) => onChange({ ...form, context_window: e.target.value })} />
          </div>
          {/* Caps */}
          <div className="col-span-2">
            <div className="mb-2 text-sm font-medium text-ink">{t("modelsPanel.caps")}</div>
            <div className="flex flex-wrap gap-2">
              {/* Reasoning */}
              <label className={["flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition select-none", form.reasoning ? "border-ink bg-ink text-background" : "border-border/60 bg-background/40 text-muted hover:border-ink/40 hover:text-ink"].join(" ")}>
                <input type="checkbox" className="hidden" checked={form.reasoning} onChange={(e) => onChange({ ...form, reasoning: e.target.checked })} />
                <Brain size={13} />
                {t("modelsPanel.capReasoning")}
              </label>
              {/* Input kinds */}
              {INPUT_KINDS.map((kind) => {
                const checked = form.input.includes(kind);
                const KindIcon = INPUT_KIND_ICONS[kind];
                return (
                  <label key={kind} className={["flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition select-none", checked ? "border-ink bg-ink text-background" : "border-border/60 bg-background/40 text-muted hover:border-ink/40 hover:text-ink"].join(" ")}>
                    <input
                      type="checkbox"
                      className="hidden"
                      checked={checked}
                      onChange={(e) => {
                        const next = e.target.checked ? [...form.input, kind] : form.input.filter((k) => k !== kind);
                        onChange({ ...form, input: next });
                      }}
                    />
                    <KindIcon size={13} />
                    {kind}
                  </label>
                );
              })}
            </div>
          </div>
        </div>
        {error && <div className="mt-4 rounded-[1rem] border border-danger/20 bg-danger/8 px-4 py-3 text-sm text-danger">{error}</div>}
        <div className="mt-6 flex justify-end gap-3">
          <Button variant="ghost" onClick={onClose} type="button">{t("modelsPanel.cancel")}</Button>
          <Button onClick={onSave} disabled={saving || !form.id.trim()} type="button">
            {saving ? t("modelsPanel.saving") : t("modelsPanel.save")}
          </Button>
        </div>
      </Panel>
    </div>
  );
}

// ─── Provider tab content ─────────────────────────────────────────────────────

interface ProviderTabProps {
  providerKey: ProviderKey;
  config: ProviderConfig;
  keyConfigured: boolean;
  onChange: (patch: Partial<ProviderConfig>) => void;
}

function ProviderTab({ config, keyConfigured, onChange }: ProviderTabProps) {
  const { t } = useTranslation("admin");
  const [modalMode, setModalMode] = useState<"add" | "edit" | null>(null);
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [modelForm, setModelForm] = useState<ModelFormState>(emptyModelForm);
  const [modelFormError, setModelFormError] = useState("");

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
        <ModelModal
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

export function AdminModelsSection() {
  const { t } = useTranslation("admin");
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

  if (isLoading || !draft) {
    return (
      <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
        <div className="text-sm text-muted">{t("modelsPanel.loading")}</div>
      </Panel>
    );
  }

  if (error) {
    return (
      <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
        <div className="text-sm text-danger">{parseErrorMessage(error)}</div>
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
            className="rounded-lg border border-border/70 bg-background/60 px-3 py-1.5 text-sm text-ink focus:outline-none"
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
            className="rounded-lg border border-border/70 bg-background/60 px-3 py-1.5 text-sm text-ink focus:outline-none"
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
