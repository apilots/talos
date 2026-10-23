import { useRef, useState } from "react";
import type { LlmProtocol, ProviderConfig, ProviderProtocolCapabilities } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";
import { Brain, Pencil, Plus, Trash2 } from "lucide-react";

import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { SettingsProviderKeyField } from "../provider-config/SettingsProviderConfigPrimitives";
import { SettingsModelModal } from "./SettingsModelModal";
import { SettingsModelDeleteDialog } from "./SettingsModelDeleteDialog";
import { incompatibleReasoningReplayModel } from "./providerProtocol";
import { parseRequestParameters } from "./requestParameters";
import { INPUT_KIND_ICONS, INPUT_KINDS, emptyModelForm, formToModel, modelToForm, type ModelFormState } from "./modelForm";

function fmtContext(n: number): string {
  if (n >= 1000) return `${Math.round(n / 1000)}K`;
  return String(n);
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

interface SettingsProviderModelsProps {
  config: ProviderConfig;
  capabilities: ProviderProtocolCapabilities;
  keyConfigured: boolean;
  custom?: boolean;
  onChange: (patch: Partial<ProviderConfig>) => void;
}

export function SettingsProviderModels({ config, capabilities, keyConfigured, custom = false, onChange }: SettingsProviderModelsProps) {
  const { t } = useTranslation("settings");
  const [modalMode, setModalMode] = useState<"add" | "edit" | null>(null);
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [modelForm, setModelForm] = useState<ModelFormState>(emptyModelForm);
  const [modelFormError, setModelFormError] = useState("");
  const [modelToDelete, setModelToDelete] = useState<string | null>(null);
  const addModelRef = useRef<HTMLButtonElement>(null);
  const [protocolConflictModel, setProtocolConflictModel] = useState<string | null>(null);
  const protocols = capabilities.protocols;
  const protocol = config.protocol ?? capabilities.default_protocol;

  const allowReasoningReplay = custom && protocol === "chat";

  function changeProtocol(next: LlmProtocol) {
    const incompatible = custom ? incompatibleReasoningReplayModel(config.models, next) : undefined;
    if (incompatible) { setProtocolConflictModel(incompatible.name || incompatible.id); return; }
    setProtocolConflictModel(null);
    onChange({ protocol: next });
  }

  function openAddModal() { setModalMode("add"); setEditingIdx(null); setModelForm(emptyModelForm); setModelFormError(""); }
  function openEditModal(idx: number) { setModalMode("edit"); setEditingIdx(idx); setModelForm(modelToForm(config.models[idx])); setModelFormError(""); }
  function closeModal() { setModalMode(null); setEditingIdx(null); setModelFormError(""); }

  function saveModel() {
    if (!modelForm.id.trim()) { setModelFormError(t("modelsPanel.modelIdRequired")); return; }
    if (!allowReasoningReplay && modelForm.reasoning_replay !== "protocol_default") { setModelFormError(t("modelsPanel.reasoningReplayOnlyCustomChat")); return; }
    const parameters = parseRequestParameters(modelForm.request_parameters);
    if (parameters.error) { setModelFormError(t(`modelsPanel.parameterError_${parameters.error}`)); return; }
    const models = [...config.models];
    const updated = formToModel(modelForm, editingIdx != null ? config.models[editingIdx] : undefined);
    if (editingIdx == null && models.some((m) => m.id === updated.id)) { setModelFormError(t("modelsPanel.modelIdDuplicate")); return; }
    if (editingIdx != null) { models[editingIdx] = updated; } else { models.push(updated); }
    onChange({ models });
    closeModal();
  }

  function deleteModel() {
    onChange({ models: config.models.filter((model) => model.id !== modelToDelete) });
    setModelToDelete(null);
  }

  return (
    <>
      <div className="mb-5 space-y-3">
        <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
          <span className="w-24 text-sm text-muted">{t("modelsPanel.protocol")}</span>
          {protocols.length > 1 ? (
            <select
              value={protocol}
              aria-label={t("modelsPanel.protocol")}
              onChange={(e) => changeProtocol(e.target.value as LlmProtocol)}
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

        {protocolConflictModel && <div className="text-sm text-danger" role="alert">{t("modelsPanel.reasoningReplayProtocolConflict", { model: protocolConflictModel })}</div>}

        <SettingsProviderKeyField
          clearLabel={t("modelsPanel.clearApiKey")}
          configured={keyConfigured}
          label={t("modelsPanel.apiKey")}
          onChange={(value) => onChange({ api_key: value })}
          onClear={() => onChange({ api_key: "" })}
          value={config.api_key ?? ""}
        />

        {/* Base URL */}
        <div className="flex items-center gap-4 rounded-[1.2rem] border border-border/70 bg-background/40 px-4 py-3">
          <span className="w-24 shrink-0 text-sm text-muted">{t("modelsPanel.baseUrl")}</span>
          <Input
            value={config.base_url ?? ""}
            onChange={(e) => onChange({ base_url: e.target.value || undefined })}
            placeholder="https://api.openai.com/v1"
            className="flex-1 text-sm font-mono"
          />
        </div>
      </div>

      {/* Models table */}
      <div className="mb-3 flex items-center justify-between">
        <div className="text-sm font-medium text-ink">{t("modelsPanel.modelsList")}</div>
        <Button variant="secondary" onClick={openAddModal} ref={addModelRef} type="button">
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
                  <button aria-label={t("modelsPanel.editModelTitle")} className="inline-flex h-7 w-7 items-center justify-center rounded-full text-muted transition hover:bg-background hover:text-ink" onClick={(event) => { event.currentTarget.focus(); openEditModal(idx); }} type="button">
                    <Pencil size={13} />
                  </button>
                  <button aria-label={t("modelsPanel.deleteModel")} className="inline-flex h-7 w-7 items-center justify-center rounded-full text-muted transition hover:bg-danger/10 hover:text-danger" onClick={(event) => { event.currentTarget.focus(); setModelToDelete(model.id); }} type="button">
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {modelToDelete && (
        <SettingsModelDeleteDialog
          fallbackFocusRef={addModelRef}
          kind="model"
          name={config.models.find((model) => model.id === modelToDelete)?.name || modelToDelete}
          onClose={() => setModelToDelete(null)}
          onConfirm={deleteModel}
        />
      )}
      {modalMode && (
        <SettingsModelModal
          mode={modalMode}
          allowReasoningReplay={allowReasoningReplay}
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

