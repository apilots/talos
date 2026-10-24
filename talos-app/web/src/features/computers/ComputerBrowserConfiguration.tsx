import { selectionClasses } from "@/shared/ui/controlStyles";
import { useState } from "react";
import {Globe2, Loader2, Monitor, PanelTop} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { BrowserOperation } from "@talos/sdk-browser";
import {useBrowser} from "./browser-query";
import { getTalosClient } from "@/shared/api/client";
import { apiErrorMessage, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import {Badge} from "@/shared/ui/Badge";
import {cn} from "@/shared/lib/cn";

/** Computer-scoped preferences; the MCP list owns plugin enablement only. */
export function ComputerBrowserConfiguration({nodeId}: {nodeId:string}) {
  const { t } = useTranslation("computers");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const query = useBrowser(nodeId);
  const browser = query.data;
  const selected = browser?.targets.find((target) => target.targetId === browser.selectedTargetId);
  async function operate(operation: BrowserOperation) {
    setBusy(true); setError(null);
    try { await getTalosClient(nodeId).browser.operate({ operation }); await query.refetch(); }
    catch (failure) { setError(failure); await query.refetch(); }
    finally { setBusy(false); }
  }
  return <div className="mx-auto max-w-[760px] space-y-6">
    <header className="space-y-2">
      <h2 className="text-xl font-semibold tracking-tight text-ink">{t("desktop.configuration.title")}</h2>
      <p className="max-w-xl text-sm leading-6 text-muted">{t("desktop.configuration.detail")}</p>
    </header>
    {query.isLoading ? <p className="text-sm text-muted">{t("desktop.configuration.loading")}</p> : null}
    {query.error || error ? <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(error ?? query.error, t("desktop.configuration.loadError"))}</p> : null}
    {browser ? <>
      {!browser.enabled ? <p className="text-sm text-muted">{t("desktop.configuration.disabled")}</p> : null}
      <fieldset disabled={busy || !browser.enabled} className="min-w-0 space-y-3 disabled:opacity-60">
        <legend className="mb-3 text-sm font-semibold text-ink">{t("desktop.configuration.target")}</legend>
        <div className="overflow-hidden rounded-[1.05rem] border border-border/80 bg-card/45">
          {browser.targets.map((target) => {
            const chosen=browser.selectedTargetId===target.targetId;
            const Icon=target.kind==="connected" ? Globe2 : browser.host==="desktop" ? PanelTop : Monitor;
            return <label key={target.targetId} className={cn("flex cursor-pointer items-start gap-3 border-b border-border/70 p-4 text-sm transition-colors last:border-0 hover:bg-hover",chosen&&selectionClasses.selected,busy||!browser.enabled?"cursor-default":"")}>
              <input type="radio" name="browser-target" className="mt-1 accent-brand" checked={chosen}
                onChange={() => void operate({ kind: "selectTarget", targetId: target.targetId, expectedRevision: browser.control.revision })} />
              <div className="min-w-0 flex-1 space-y-2">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="inline-flex items-center gap-2 font-medium text-ink"><Icon aria-hidden="true" size={15} className="shrink-0 text-muted"/>{t(`desktop.configuration.${target.kind}`)}</span>
                  <Badge tone={target.readiness.status==="ready"?"success":target.readiness.status==="unavailable"?"danger":"default"} className="shrink-0 px-2 py-0.5 normal-case tracking-normal">{t(`desktop.configuration.readiness.${target.readiness.status}`)}</Badge>
                </div>
                <p className="text-xs leading-5 text-muted">{t(target.kind === "connected" ? "desktop.configuration.connectedDetail" : browser.host === "desktop" ? "desktop.configuration.managedLocal" : "desktop.configuration.managedDocker")}</p>
              </div>
            </label>;
          })}
        </div>
      </fieldset>
      {selected?.readiness.status === "unavailable" ? <p className="text-sm text-danger">{apiErrorMessage(selected.readiness.error)}</p> : null}
      {selected?.kind === "connected" ? <div className="space-y-3">
        <details className="rounded-xl border border-border/70 bg-surface/40 px-4 py-3 text-xs text-muted">
          <summary className="cursor-pointer font-medium text-ink">{t("desktop.configuration.chromeRequirements")}</summary>
          <p className="mt-2 break-words leading-5">{t("desktop.configuration.chromeHelp")}</p>
        </details>
        <Button variant="secondary" disabled={busy || !browser.enabled} onClick={() => void operate({ kind: selected.readiness.status === "ready" ? "disconnect" : "connect", expectedRevision: browser.control.revision })}>
          {busy ? <Loader2 aria-hidden="true" size={13} className="mr-1.5 animate-spin motion-reduce:animate-none"/> : null}
          {t(selected.readiness.status === "ready" ? "desktop.configuration.disconnect" : "desktop.configuration.connect")}
        </Button>
      </div> : null}
      {browser.targets.length === 0 ? <p className="text-sm text-muted">{t("desktop.configuration.unavailable")}</p> : null}
    </> : null}
  </div>;
}
