import { overlayClasses } from "@/shared/ui/overlayStyles";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-core";
import { getTalosClient } from "@/shared/api/client";
import { useAppStore } from "@/shared/store/app-store";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import { useProductScope } from "../hooks";

export function ComputerRegistrationActions({ node, pending, onToggleAccess, onDeleted }: {
  node: AppProtocol.NodeInfo;
  pending: boolean;
  onToggleAccess: () => void;
  onDeleted: () => void;
}) {
  const { t } = useTranslation(["computers", "common"]);
  const [confirming, setConfirming] = useState(false);
  const scope = useProductScope();
  const queries = useQueryClient();
  const cancel = useRef<HTMLButtonElement>(null);
  const mutation = useMutation({
    mutationFn: () => getTalosClient().computers.delete({ nodeId: node.nodeId }),
    onSuccess: () => {
      useAppStore.getState().clearComputer(scope, node.nodeId);
      queries.removeQueries({ queryKey: ["talos", scope, "computer", node.nodeId], exact: true });
      setConfirming(false);
      onDeleted();
      void queries.invalidateQueries({ queryKey: ["talos", scope] });
    },
  });
  useEffect(() => {
    if (!confirming) return;
    const previous = document.activeElement;
    cancel.current?.focus();
    return () => { if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  }, [confirming]);
  const close = () => { if (!mutation.isPending) { setConfirming(false); mutation.reset(); } };
  return (
    <div className="cmp-registration-actions">
      <Button variant="secondary" disabled={pending || mutation.isPending} onClick={onToggleAccess}>
        {t(node.status === "enabled" ? "computers:disable" : "computers:enable")}
      </Button>
      <Button variant="danger" disabled={pending || mutation.isPending} onClick={() => setConfirming(true)}>
        {t("computers:removeRegistration")}
      </Button>
      {confirming ? createPortal(
        <div className={`fixed inset-0 z-[1000] flex items-center justify-center px-4 ${overlayClasses.tint}`} onClick={(event) => { if (event.target === event.currentTarget) close(); }}>
          <div role="dialog" aria-modal="true" aria-labelledby="computer-remove-title" aria-describedby="computer-remove-description"
            className="w-full max-w-lg rounded-[var(--radius-panel)] border border-border bg-card p-6 shadow-panel"
            onKeyDown={(event) => {
              if (event.key === "Escape") { event.preventDefault(); close(); }
              if (event.key === "Tab") {
                const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
                const first = buttons[0]; const last = buttons[buttons.length - 1];
                if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
                else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
              }
            }}>
            <h2 id="computer-remove-title" className="text-xl font-semibold text-ink">{t("computers:removeTitle", { name: node.displayName })}</h2>
            <p id="computer-remove-description" className="mt-4 text-sm leading-6 text-muted">{t("computers:removeDescription")}</p>
            <p className="mt-3 text-sm leading-6 text-muted">{t("computers:removeRetention")}</p>
            {mutation.error ? <p role="alert" className="mt-4 text-sm text-danger">{userVisibleErrorMessage(mutation.error, t("computers:removeError"))}</p> : null}
            <div className="mt-6 flex justify-end gap-3">
              <button ref={cancel} type="button" disabled={mutation.isPending} onClick={close} className="rounded-[var(--radius-control)] px-3.5 py-2.5 text-sm text-muted hover:bg-hover">{t("common:cancel")}</button>
              <Button variant="danger" disabled={mutation.isPending} onClick={() => mutation.mutate()}>{t(mutation.isPending ? "computers:removing" : "computers:removeRegistration")}</Button>
            </div>
          </div>
        </div>, document.body,
      ) : null}
    </div>
  );
}
