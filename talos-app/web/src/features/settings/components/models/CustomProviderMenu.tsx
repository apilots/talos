import { useRef, type RefObject } from "react";
import { Check, ChevronDown, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { CustomProviderConfig } from "@talos/sdk-browser";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/shared/ui/DropdownMenu";

export function CustomProviderMenu({ providers, selected, onSelect, onAdd, triggerRef }: {
  providers: Record<string, CustomProviderConfig | undefined>;
  selected: string;
  onSelect: (id: string) => void;
  onAdd: () => void;
  triggerRef?: RefObject<HTMLButtonElement | null>;
}) {
  const { t } = useTranslation("settings");
  const internalTrigger = useRef<HTMLButtonElement>(null);
  const trigger = triggerRef ?? internalTrigger;
  const opensDialog = useRef(false);
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger asChild><button className={["flex min-w-0 flex-1 items-center justify-center gap-1 rounded-xl px-3 py-2 text-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-brand/20", providers[selected] ? "bg-card text-ink shadow-sm" : "text-muted hover:text-ink"].join(" ")} ref={trigger} type="button">{t("modelsPanel.customProviders")}<ChevronDown aria-hidden size={13} /></button></DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56" onCloseAutoFocus={(event) => { if (opensDialog.current) { event.preventDefault(); opensDialog.current = false; } }}>
        {Object.entries(providers).map(([id, provider]) => provider && <DropdownMenuItem key={id} onSelect={() => onSelect(id)}><Check aria-hidden className={id === selected ? "shrink-0" : "invisible shrink-0"} size={14} /><span className="min-w-0 break-words">{provider.name}</span></DropdownMenuItem>)}
        {Object.keys(providers).length > 0 && <DropdownMenuSeparator className="my-1 border-t border-border/60" />}
        <DropdownMenuItem onSelect={() => { opensDialog.current = true; trigger.current?.focus(); onAdd(); }}><Plus aria-hidden size={14} />{t("modelsPanel.addProvider")}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
