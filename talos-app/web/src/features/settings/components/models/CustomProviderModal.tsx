import { useState } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { CustomProviderConfig, LlmProtocol } from "@talos/sdk-browser";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Select } from "@/shared/ui/Select";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@/shared/ui/Dialog";
import { CUSTOM_PROTOCOLS, PROTOCOL_LABEL_KEYS, incompatibleReasoningReplayModel } from "./providerProtocol";
import { useSettingsDialogFocus } from "./useSettingsDialogFocus";

export function CustomProviderModal({ initial, onSave, onClose }: {
  initial?: CustomProviderConfig;
  onSave: (provider: CustomProviderConfig) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("settings");
  const returnFocus = useSettingsDialogFocus();
  const [name, setName] = useState(initial?.name ?? "");
  const [protocol, setProtocol] = useState<LlmProtocol>(initial?.protocol ?? "chat");
  const [baseUrl, setBaseUrl] = useState(initial?.base_url ?? "");
  const [apiKey, setApiKey] = useState(initial?.api_key ?? "");
  const [error, setError] = useState<"fields" | "replay" | null>(null);
  const incompatibleModel = incompatibleReasoningReplayModel(initial?.models ?? [], protocol);
  function save() {
    try {
      const url = new URL(baseUrl.trim());
      if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.search || url.hash || !name.trim()) throw new Error();
    } catch { setError("fields"); return; }
    if (incompatibleModel) { setError("replay"); return; }
    onSave({ ...initial, name: name.trim(), protocol, base_url: baseUrl.trim(), api_key: apiKey, models: initial?.models ?? [] });
  }
  return (
    <Dialog onOpenChange={(open) => { if (!open) onClose(); }} open>
      <DialogContent onCloseAutoFocus={returnFocus}>
        <DialogTitle className="pr-10 text-lg font-semibold">{t(initial ? "modelsPanel.editProvider" : "modelsPanel.addProvider")}</DialogTitle>
        <DialogDescription className="mt-1 text-sm text-muted">{t("modelsPanel.providerDetail")}</DialogDescription>
        <form className="mt-5" onSubmit={(event) => { event.preventDefault(); save(); }}>
          <div className="space-y-4">
            <div>
              <label className="mb-1.5 block text-sm font-medium" htmlFor="custom-provider-name">{t("modelsPanel.modelName")}</label>
              <Input id="custom-provider-name" onChange={(event) => setName(event.target.value)} placeholder="LLM Gateway" value={name} />
            </div>
            <div>
              <label className="mb-1.5 block text-sm font-medium" htmlFor="custom-provider-api">{t("modelsPanel.apiFormat")}</label>
              <Select id="custom-provider-api" onValueChange={(value) => { setProtocol(value as LlmProtocol); setError(null); }} options={CUSTOM_PROTOCOLS.map((value) => ({ value, label: t(PROTOCOL_LABEL_KEYS[value]) }))} value={protocol} />
            </div>
            <div>
              <label className="mb-1.5 block text-sm font-medium" htmlFor="custom-provider-url">{t("modelsPanel.baseUrl")}</label>
              <Input autoCapitalize="off" autoComplete="off" className="font-mono" id="custom-provider-url" onChange={(event) => setBaseUrl(event.target.value)} placeholder="https://llm.example.com/v1" spellCheck={false} value={baseUrl} />
            </div>
            <div>
              <label className="mb-1.5 block text-sm font-medium" htmlFor="custom-provider-key">{t("modelsPanel.apiKey")} <span className="font-normal text-muted">{t("modelsPanel.optional")}</span></label>
              <Input autoComplete="off" id="custom-provider-key" onChange={(event) => setApiKey(event.target.value)} type="password" value={apiKey} />
            </div>
          </div>
          {error && <div className="mt-3 text-sm text-danger" role="alert">{t(error === "fields" ? "modelsPanel.providerFieldsRequired" : "modelsPanel.reasoningReplayProtocolConflict", { model: incompatibleModel?.name || incompatibleModel?.id })}</div>}
          <div className="mt-6 flex justify-end gap-3">
            <DialogClose asChild><Button type="button" variant="ghost">{t("modelsPanel.cancel")}</Button></DialogClose>
            <Button disabled={!name.trim() || !baseUrl.trim()} type="submit">{t("modelsPanel.save")}</Button>
          </div>
        </form>
        <DialogClose asChild><button aria-label={t("common:close")} className="absolute right-5 top-5 flex size-8 items-center justify-center rounded-full text-muted outline-none hover:bg-surface focus-visible:ring-2 focus-visible:ring-brand/20" type="button"><X aria-hidden size={18} /></button></DialogClose>
      </DialogContent>
    </Dialog>
  );
}
