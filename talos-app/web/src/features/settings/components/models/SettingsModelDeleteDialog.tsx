import type { RefObject } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/shared/ui/Button";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogTitle } from "@/shared/ui/Dialog";
import { useSettingsDialogFocus } from "./useSettingsDialogFocus";

interface SettingsModelDeleteDialogProps {
  kind: "provider" | "model";
  name: string;
  fallbackFocusRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  onConfirm: () => void;
}

export function SettingsModelDeleteDialog({ kind, name, fallbackFocusRef, onClose, onConfirm }: SettingsModelDeleteDialogProps) {
  const { t } = useTranslation(["settings", "common"]);
  const returnFocus = useSettingsDialogFocus(fallbackFocusRef);
  return (
    <AlertDialog onOpenChange={(open) => { if (!open) onClose(); }} open>
      <AlertDialogContent onCloseAutoFocus={returnFocus}>
        <AlertDialogTitle className="break-words pr-9 text-xl font-semibold">{t(kind === "provider" ? "modelsPanel.deleteProviderTitle" : "modelsPanel.deleteModelTitle", { name })}</AlertDialogTitle>
        <AlertDialogDescription className="mt-3 text-sm leading-6 text-muted">{t(kind === "provider" ? "modelsPanel.deleteProviderDetail" : "modelsPanel.deleteModelDetail")}</AlertDialogDescription>
        <AlertDialogCancel asChild><button aria-label={t("common:close")} className="absolute right-5 top-5 flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted outline-none hover:bg-surface focus-visible:ring-2 focus-visible:ring-brand/30" type="button"><X aria-hidden size={18} /></button></AlertDialogCancel>
        <div className="mt-7 flex justify-end gap-3">
          <AlertDialogCancel asChild><Button className="min-w-24" type="button" variant="ghost">{t("common:cancel")}</Button></AlertDialogCancel>
          <AlertDialogAction asChild><Button className="min-w-24" onClick={onConfirm} type="button" variant="danger">{t("common:delete")}</Button></AlertDialogAction>
        </div>
      </AlertDialogContent>
    </AlertDialog>
  );
}
