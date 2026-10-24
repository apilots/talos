import type { ReactNode } from "react";
import { ChevronDown, Plus, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Textarea } from "@/shared/ui/Input";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/shared/ui/DropdownMenu";
import type { RequestParameterRow } from "./requestParameters";

const EXAMPLES = {
  vllm: { key: "chat_template_kwargs", value: '{"enable_thinking":true}' },
  thinking: { key: "enable_thinking", value: "true" },
  budget: { key: "thinking_budget", value: "2048" },
} satisfies Record<string, RequestParameterRow>;

export function RequestParametersEditor({ rows, onChange, children }: {
  rows: RequestParameterRow[];
  children?: ReactNode;
  onChange: (rows: RequestParameterRow[]) => void;
}) {
  const { t } = useTranslation("settings");
  const exists = (key: string) => rows.some((row) => row.key.trim() === key);
  function applyExample(example: RequestParameterRow) {
    if (!exists(example.key)) onChange([...rows, { ...example }]);
  }
  const examples = [
    { example: EXAMPLES.vllm, label: t("modelsPanel.parameterExampleTemplateThinking"), path: "chat_template_kwargs.enable_thinking" },
    { example: EXAMPLES.thinking, label: t("modelsPanel.parameterExampleTopLevelThinking"), path: t("modelsPanel.parameterTopLevel", { key: "enable_thinking" }) },
    { example: EXAMPLES.budget, label: "thinking_budget", path: t("modelsPanel.parameterTopLevel", { key: "thinking_budget" }) },
  ];
  return (
    <details className="group mt-5 border-t border-border/60 pt-4">
      <summary className="flex cursor-pointer list-none items-center gap-2 text-sm font-medium text-ink [&::-webkit-details-marker]:hidden">
        <ChevronDown aria-hidden className="-rotate-90 transition group-open:rotate-0" size={14} />{t("modelsPanel.advanced")}
      </summary>
      {children}
      <div className="mb-1 mt-4 flex items-center justify-between gap-2">
        <span className="text-sm font-medium text-ink">{t("modelsPanel.requestParameters")}</span>
        <DropdownMenu modal={false}>
          <DropdownMenuTrigger asChild><button className="inline-flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-xs text-muted outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-brand/20" type="button">{t("modelsPanel.parameterExamples")}<ChevronDown aria-hidden size={13} /></button></DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-72">
            {examples.map(({ example, label, path }) => <DropdownMenuItem aria-label={label} disabled={exists(example.key)} key={example.key} onSelect={() => applyExample(example)}>
              <div className="min-w-0"><div>{label}</div><div className="mt-0.5 break-words font-mono text-[11px] text-muted">{path}</div>{exists(example.key) && <div className="mt-1 text-xs text-muted">{t("modelsPanel.parameterExampleExists")}</div>}</div>
            </DropdownMenuItem>)}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <p className="mb-3 text-xs text-muted">{t("modelsPanel.parameterHint")}</p>
      {rows.length > 0 && <div className="mb-1 hidden grid-cols-[minmax(0,170px)_minmax(0,1fr)_24px] gap-2 px-2 text-xs text-muted sm:grid"><span>{t("modelsPanel.parameterKeyLabel")}</span><span>{t("modelsPanel.parameterValueLabel")}</span></div>}
      {rows.map((row, index) => (
        <div className="mb-3 grid grid-cols-[minmax(0,1fr)_24px] items-start gap-2 sm:grid-cols-[minmax(0,170px)_minmax(0,1fr)_24px]" key={index}>
          <Textarea aria-label={t("modelsPanel.parameterKey", { index: index + 1 })} className="min-h-10 min-w-0 resize-none px-2 py-2 font-mono text-xs [field-sizing:content] [overflow-wrap:anywhere]" onChange={(event) => onChange(rows.map((item, i) => i === index ? { ...item, key: event.target.value } : item))} placeholder={t("modelsPanel.parameterKeyLabel")} rows={1} spellCheck={false} title={row.key} value={row.key} />
          <Textarea aria-label={t("modelsPanel.parameterValue", { index: index + 1 })} className="col-span-2 max-h-48 min-h-10 min-w-0 resize-y whitespace-pre-wrap px-2 py-2 font-mono text-xs [field-sizing:content] [overflow-wrap:anywhere] sm:col-span-1" onChange={(event) => onChange(rows.map((item, i) => i === index ? { ...item, value: event.target.value } : item))} placeholder="true / 2048 / {}" rows={2} spellCheck={false} value={row.value} wrap="soft" />
          <button aria-label={t("modelsPanel.removeParameter", { index: index + 1 })} className="col-start-2 row-start-1 mt-1 flex size-6 items-center justify-center rounded-full text-muted outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-brand/20 sm:col-start-3" onClick={() => onChange(rows.filter((_, i) => i !== index))} type="button"><X aria-hidden size={14} /></button>
        </div>
      ))}
      <button className="inline-flex items-center gap-1.5 rounded py-1 text-xs text-muted outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/20" onClick={() => onChange([...rows, { key: "", value: "" }])} type="button"><Plus aria-hidden size={13} />{t("modelsPanel.addParameter")}</button>
    </details>
  );
}
