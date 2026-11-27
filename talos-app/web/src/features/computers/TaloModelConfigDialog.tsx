import type { RefObject } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { SettingsModelsSection } from "@/features/settings";
import { Button } from "@/shared/ui/Button";
import { Dialog, DialogClose, DialogContent, DialogTitle } from "@/shared/ui/Dialog";
import { useDialogFocusReturn } from "@/shared/ui/useDialogFocusReturn";

/** Mount on open so focus returns to the launcher in either the chat or console. */
export function TaloModelConfigDialog({ nodeId, fallbackFocusRef, onClose }: {
  nodeId: string;
  fallbackFocusRef: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const { t } = useTranslation("computers");
  const returnFocus = useDialogFocusReturn(fallbackFocusRef);

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent
        aria-describedby={undefined}
        className="max-h-[88dvh] max-w-[820px] overflow-y-auto"
        onCloseAutoFocus={returnFocus}
      >
        <div className="mb-4 flex items-center justify-between gap-3">
          <DialogTitle className="text-base font-semibold text-ink">
            {t("talo.modelConfig")}
          </DialogTitle>
          <DialogClose asChild>
            <Button type="button" variant="ghost" className="size-8 p-0" aria-label={t("talo.closeModelConfig")}>
              <X size={16} aria-hidden="true" />
            </Button>
          </DialogClose>
        </div>
        <SettingsModelsSection nodeId={nodeId} />
      </DialogContent>
    </Dialog>
  );
}
