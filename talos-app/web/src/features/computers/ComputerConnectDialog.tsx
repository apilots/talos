import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/shared/ui/Button";

export function ComputerConnectDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation(["computers", "common"]);
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current!;
    const trigger = document.activeElement;
    dialog.showModal();
    return () => {
      const restore = document.activeElement === document.body || dialog.contains(document.activeElement);
      dialog.close();
      if (restore && trigger instanceof HTMLElement && trigger.isConnected) trigger.focus();
    };
  }, []);

  return (
    <dialog ref={ref} aria-labelledby="computer-connect-title" onCancel={(event) => { event.preventDefault(); onClose(); }}
      onClick={(event) => {
        if (event.target !== event.currentTarget) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) onClose();
      }} className="m-auto w-[min(420px,calc(100vw-40px))] rounded-[var(--radius-panel)] border border-border bg-card p-6 text-ink shadow-panel backdrop:bg-black/15">
      <div className="mb-5 flex items-center justify-between gap-3">
        <h2 id="computer-connect-title" className="text-lg font-medium">{t("computers:add")}</h2>
        <Button variant="ghost" className="p-1.5" aria-label={t("common:close")} onClick={onClose}><X size={16} aria-hidden="true" /></Button>
      </div>
      <p className="text-sm leading-7 text-muted">{t("computers:home.connectHelp")}</p>
      <p className="mt-5 border-t border-border pt-4 text-xs leading-6 text-muted">{t("computers:home.appearsAutomatically")}</p>
      <div className="mt-6 flex justify-end"><Button onClick={onClose}>{t("computers:home.gotIt")}</Button></div>
    </dialog>
  );
}
