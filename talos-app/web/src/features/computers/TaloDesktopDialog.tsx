import { useRef, type RefObject } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";
import { Dialog, DialogClose, DialogContent, DialogTitle } from "@/shared/ui/Dialog";
import { ComputerDesktop } from "./ComputerDesktop";
import { TaloEmblem } from "./talo-lifecycle";

interface TaloDesktopDialogProps {
  nodeId: string | undefined;
  open: boolean;
  returnFocusRef?: RefObject<HTMLElement | null>;
  onOpenChange: (open: boolean) => void;
}

export function TaloDesktopDialog({ nodeId, open, returnFocusRef, onOpenChange }: TaloDesktopDialogProps) {
  const { t } = useTranslation("computers");
  const closeRef = useRef<HTMLButtonElement>(null);

  if (!nodeId) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        aria-describedby={undefined}
        className="flex max-h-[92dvh] max-w-[min(960px,94vw)] flex-col overflow-hidden p-0"
        onCloseAutoFocus={(event) => {
          const target = returnFocusRef?.current;
          if (target?.isConnected && !(target as HTMLButtonElement).disabled && !target.closest("[inert], [hidden]")) {
            event.preventDefault();
            target.focus();
          }
        }}
      >
        <div className="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-divider bg-surface px-4">
          <div className="flex min-w-0 items-center gap-2.5">
            <TaloEmblem size={22} />
            <DialogTitle className="truncate text-sm font-semibold text-ink">
              {t("talo.desktopTitle")}
            </DialogTitle>
          </div>
          <DialogClose asChild>
            <Button ref={closeRef} type="button" variant="ghost" className="size-8 p-0" aria-label={t("talo.closeDesktop")}>
              <X size={16} aria-hidden="true" />
            </Button>
          </DialogClose>
        </div>
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          {open ? (
            <ComputerDesktop
              nodeId={nodeId}
              expanded
              onExitInput={() => closeRef.current?.focus()}
            />
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}
