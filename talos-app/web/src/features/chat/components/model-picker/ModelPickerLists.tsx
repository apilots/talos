import { Check, ChevronDown, ChevronLeft } from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { ComposerModelOption } from "../../model-selection";
import styles from "./ModelPicker.module.css";

export interface ModelPickerProvider { id: string; name: string; custom: boolean }

function ListHeader({ title, onBack, children }: { title: string; onBack: () => void; children?: ReactNode }) {
  const { t } = useTranslation("chat");
  return <div className={styles.listHead}><button aria-label={t("modelPicker.back")} className={styles.back} onClick={onBack} type="button"><ChevronLeft aria-hidden size={16} /></button><span className={styles.listTitle}>{title}</span>{children}</div>;
}

function Option({ name, selected, onClick }: { name: string; selected: boolean; onClick: () => void }) {
  return <button aria-pressed={selected} className={`${styles.option}${selected ? ` ${styles.selected}` : ""}`} onClick={onClick} type="button"><span>{name}</span>{selected && <Check aria-hidden size={13} />}</button>;
}

export function ModelPickerModelList({ models, providerName, selectedModel, onBack, onProvider, onSelect }: {
  models: ComposerModelOption[];
  providerName: string;
  selectedModel?: string;
  onBack: () => void;
  onProvider: () => void;
  onSelect: (model: ComposerModelOption) => void;
}) {
  const { t } = useTranslation("chat");
  return <><ListHeader onBack={onBack} title={t("modelPicker.chooseModel")}><button aria-label={t("modelPicker.changeProvider", { provider: providerName })} className={styles.providerSwitch} onClick={onProvider} type="button"><span>{providerName}</span><ChevronDown aria-hidden size={11} /></button></ListHeader><div className={styles.listContent}>{models.map((model) => <Option key={model.id} name={model.name} onClick={() => onSelect(model)} selected={selectedModel === model.id} />)}</div></>;
}

export function ModelPickerProviderList({ providers, selectedProvider, onBack, onSelect }: {
  providers: ModelPickerProvider[];
  selectedProvider?: string;
  onBack: () => void;
  onSelect: (provider: ModelPickerProvider) => void;
}) {
  const { t } = useTranslation("chat");
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => { listRef.current?.querySelector<HTMLElement>('[aria-pressed="true"]')?.scrollIntoView?.({ block: "nearest" }); }, []);
  return <><ListHeader onBack={onBack} title={t("modelPicker.chooseProvider")} /><div className={styles.listContent} ref={listRef}>{providers.map((provider, index) => <div key={provider.id}>{provider.custom && !providers[index - 1]?.custom && <div className={styles.optionGroup}>{t("modelPicker.custom")}</div>}<Option name={provider.name} onClick={() => onSelect(provider)} selected={selectedProvider === provider.id} /></div>)}</div></>;
}
