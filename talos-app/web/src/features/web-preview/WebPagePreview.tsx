import { useRef, useState } from "react";
import { ArrowRight, Globe2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useBrowserSurface } from "./useBrowserSurface";
import { BrowserToolbar, BrowserFrame } from "./BrowserToolbar";
import { normalizeWorkspaceWebpageUrl } from "@/shared/store/workspace-window";

/** A separate client-side preview, with no Talos credentials or host bridge. */
export function WebPagePreview({ url, onNavigate }: { url: string; onNavigate: (url: string, title?: string) => void }) {
  const { t } = useTranslation("webPreview");
  const [revision, setRevision] = useState(0);
  const addressInput = useRef<HTMLInputElement>(null);
  const destination = normalizeWorkspaceWebpageUrl(url);
  const surface = useBrowserSurface(destination ?? undefined, onNavigate);

  return (
    <section aria-label={t("title")} className="flex h-full min-h-0 flex-col bg-background">
      <BrowserToolbar source={{ kind: "webpage", url, onNavigate, inputRef: addressInput }} surface={surface} onRefresh={() => surface.native ? surface.command("reload") : setRevision(value => value + 1)} refreshDisabled={!destination} />
      {destination ? <p className="shrink-0 border-b border-border/50 bg-surface/20 px-4 py-1.5 text-[11px] leading-4 text-muted">{t("previewHint")}</p> : null}
      {!url ? (
        <div className="min-h-0 flex-1 overflow-y-auto bg-background px-7 py-10">
          <div className="mx-auto flex min-h-full max-w-[280px] flex-col items-center justify-center text-center">
            <div className="mb-5 flex size-12 items-center justify-center rounded-2xl border border-border/65 bg-card text-muted shadow-sm"><Globe2 aria-hidden="true" size={23} strokeWidth={1.5} /></div>
            <h1 className="font-display text-2xl font-medium tracking-[-0.025em] text-ink">{t("emptyTitle")}</h1>
            <p className="mt-2.5 text-[13px] leading-6 text-muted">{t("emptyDetail")}</p>
            <button className="mt-5 inline-flex items-center gap-2 rounded-full border border-border/80 bg-card px-4 py-2 text-xs font-medium text-ink outline-none transition hover:border-brand/30 hover:bg-hover focus-visible:ring-2 focus-visible:ring-brand/40" onClick={() => addressInput.current?.focus()} type="button">{t("enterAddress")}<ArrowRight aria-hidden="true" size={13} /></button>
            <p className="mt-7 w-full border-t border-border/60 pt-4 text-[11px] leading-5 text-muted">{t("openHint")}</p>
          </div>
        </div>
      ) : !destination ? (
        <div className="m-5 rounded-xl border border-danger/25 bg-danger/5 p-5 text-sm leading-6 text-danger" role="alert">{t("invalidAddress")}</div>
      ) : (
        <BrowserFrame key={surface.native ? "native" : `${destination}:${revision}`} surface={surface} url={destination} title={t("frameTitle", { url: destination })} />
      )}
    </section>
  );
}
