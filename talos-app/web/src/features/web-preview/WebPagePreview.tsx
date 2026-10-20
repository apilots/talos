import { useEffect, useRef, useState } from "react";
import { ArrowRight, ExternalLink, Globe2, RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { openExternalUrl } from "@/shared/lib/open-external";
import { normalizeWorkspaceWebpageUrl } from "@/shared/store/workspace-window";

/** A separate client-side preview, with no Talos credentials or host bridge. */
export function WebPagePreview({ url, onNavigate }: { url: string; onNavigate: (url: string) => void }) {
  const { t } = useTranslation("webPreview");
  const [address, setAddress] = useState(url);
  const [addressError, setAddressError] = useState(false);
  const [externalError, setExternalError] = useState(false);
  const [revision, setRevision] = useState(0);
  const addressInput = useRef<HTMLInputElement>(null);
  const destination = normalizeWorkspaceWebpageUrl(url);
  useEffect(() => {
    setAddress(url);
    setAddressError(false);
    setExternalError(false);
  }, [url]);

  return (
    <section aria-label={t("title")} className="flex h-full min-h-0 flex-col bg-card">
      <div className="shrink-0 border-b border-border/65 bg-surface/35 px-3 py-2">
        <div className="flex items-center gap-1.5">
          <button aria-label={t("refresh")} title={t("refresh")} className="flex size-8 shrink-0 items-center justify-center rounded-full text-muted outline-none transition hover:bg-selected/70 hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/40 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent" disabled={!destination} onClick={() => setRevision(value => value + 1)} type="button"><RotateCw aria-hidden="true" size={15} /></button>
          <form className="flex h-9 min-w-0 flex-1 items-center rounded-full border border-border/75 bg-card shadow-sm transition focus-within:border-brand/35 focus-within:ring-2 focus-within:ring-brand/10" onSubmit={event => {
            event.preventDefault();
            const next = normalizeWorkspaceWebpageUrl(address.trim());
            setAddressError(!next);
            if (next) {
              if (next === destination) setRevision(value => value + 1);
              onNavigate(next);
            }
          }}>
            <Globe2 aria-hidden="true" className="ml-3.5 shrink-0 text-muted/80" size={14} />
            <input aria-invalid={addressError} aria-label={t("address")} autoComplete="off" className="h-full min-w-0 flex-1 bg-transparent px-2.5 text-[13px] text-ink outline-none placeholder:text-muted/65 selection:bg-brand/15" onChange={event => { setAddress(event.target.value); setAddressError(false); }} placeholder="https://example.com" ref={addressInput} spellCheck={false} type="text" value={address} />
            <button aria-label={t("go")} title={t("go")} className="mr-1 flex size-7 shrink-0 items-center justify-center rounded-full text-muted outline-none transition hover:bg-surface hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/40" type="submit"><ArrowRight aria-hidden="true" size={14} /></button>
          </form>
          <button aria-label={t("openExternal")} title={t("openExternal")} className="flex size-8 shrink-0 items-center justify-center rounded-full text-muted outline-none transition hover:bg-selected/70 hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/40 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent" disabled={!destination} onClick={() => { setExternalError(false); if (destination) void openExternalUrl(destination).catch(() => setExternalError(true)); }} type="button"><ExternalLink aria-hidden="true" size={15} /></button>
        </div>
        {addressError ? <p className="mt-2 px-1 text-xs text-danger" role="alert">{t("invalidAddress")}</p> : null}
        {externalError ? <p className="mt-2 px-1 text-xs text-danger" role="alert">{t("externalFailed")}</p> : null}
      </div>
      {destination ? <p className="shrink-0 border-b border-border/50 bg-surface/20 px-4 py-1.5 text-[11px] leading-4 text-muted">{t("previewHint")}</p> : null}
      {!url ? (
        <div className="min-h-0 flex-1 overflow-y-auto bg-background/35 px-7 py-10">
          <div className="mx-auto flex min-h-full max-w-[280px] flex-col items-center justify-center text-center">
            <div className="mb-5 flex size-12 items-center justify-center rounded-2xl border border-border/65 bg-card text-muted shadow-sm"><Globe2 aria-hidden="true" size={23} strokeWidth={1.5} /></div>
            <h1 className="font-display text-2xl font-medium tracking-[-0.025em] text-ink">{t("emptyTitle")}</h1>
            <p className="mt-2.5 text-[13px] leading-6 text-muted">{t("emptyDetail")}</p>
            <button className="mt-5 inline-flex items-center gap-2 rounded-full border border-border/80 bg-card px-4 py-2 text-xs font-medium text-ink outline-none transition hover:border-brand/30 hover:bg-surface focus-visible:ring-2 focus-visible:ring-brand/40" onClick={() => addressInput.current?.focus()} type="button">{t("enterAddress")}<ArrowRight aria-hidden="true" size={13} /></button>
            <p className="mt-7 w-full border-t border-border/60 pt-4 text-[11px] leading-5 text-muted">{t("openHint")}</p>
          </div>
        </div>
      ) : !destination ? (
        <div className="m-5 rounded-xl border border-danger/25 bg-danger/5 p-5 text-sm leading-6 text-danger" role="alert">{t("invalidAddress")}</div>
      ) : (
        <iframe
          className="min-h-0 flex-1 border-0 bg-white"
          key={`${destination}:${revision}`}
          referrerPolicy="no-referrer"
          sandbox="allow-scripts allow-forms"
          src={destination}
          title={t("frameTitle", { url: destination })}
        />
      )}
    </section>
  );
}
