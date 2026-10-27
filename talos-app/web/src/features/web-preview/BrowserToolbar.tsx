import { useEffect, useState, type ReactNode, type RefObject } from "react";
import { ArrowRight, Globe2, RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { normalizeWorkspaceWebpageUrl } from "@/shared/store/workspace-window";
import { ResourceOpenMenu } from "./ResourceOpenMenu";

export type BrowserAddress = { kind: "webpage"; url: string; onNavigate: (url: string) => void; inputRef?: RefObject<HTMLInputElement | null> }
  | { kind: "file"; label: string; fileId: string };
const iconButton = "flex size-8 shrink-0 items-center justify-center rounded-full text-muted outline-none transition hover:bg-hover hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/40 disabled:cursor-not-allowed disabled:opacity-35";

/** Shared browser chrome; file locations are descriptive, never arbitrary filesystem inputs. */
export function BrowserToolbar({ source, onRefresh, refreshDisabled, children }: {
  source: BrowserAddress; onRefresh: () => void; refreshDisabled?: boolean; children?: ReactNode;
}) {
  const { t } = useTranslation("webPreview");
  const location = source.kind === "webpage" ? source.url : source.label;
  const [address, setAddress] = useState(location);
  const [invalidAddress, setInvalidAddress] = useState(false);
  useEffect(() => { setAddress(location); setInvalidAddress(false); }, [location]);
  const destination = source.kind === "webpage" ? normalizeWorkspaceWebpageUrl(source.url) : null;
  return <div className="shrink-0 border-b border-divider bg-surface px-3 py-2">
    <div className="flex items-center gap-1.5">
      <button aria-label={t("refresh")} title={t("refresh")} className={iconButton} disabled={refreshDisabled} onClick={onRefresh} type="button"><RotateCw aria-hidden="true" size={15} /></button>
      <form className="flex h-9 min-w-0 flex-1 items-center rounded-full border border-border/75 bg-card shadow-sm focus-within:border-brand/35 focus-within:ring-2 focus-within:ring-brand/10" onSubmit={event => {
        event.preventDefault();
        if (source.kind !== "webpage") return;
        const next = normalizeWorkspaceWebpageUrl(address.trim());
        setInvalidAddress(!next);
        if (next) { if (next === destination) onRefresh(); source.onNavigate(next); }
      }}>
        <Globe2 aria-hidden="true" className="ml-3.5 shrink-0 text-muted/80" size={14} />
        <input aria-invalid={invalidAddress} aria-label={t(source.kind === "file" ? "fileAddress" : "address")} autoComplete="off" className="h-full min-w-0 flex-1 bg-transparent px-2.5 text-[13px] text-ink outline-none placeholder:text-muted/65" onChange={event => { setAddress(event.target.value); setInvalidAddress(false); }} placeholder="https://example.com" readOnly={source.kind === "file"} ref={source.kind === "webpage" ? source.inputRef : undefined} spellCheck={false} value={address} />
        {source.kind === "webpage" ? <button aria-label={t("go")} title={t("go")} className="mr-1 flex size-7 shrink-0 items-center justify-center rounded-full text-muted hover:bg-hover focus-visible:ring-2 focus-visible:ring-brand/40" type="submit"><ArrowRight aria-hidden="true" size={14} /></button> : null}
      </form>
      {source.kind === "file" ? <ResourceOpenMenu resource={{ kind: "file", fileId: source.fileId }} /> : destination ? <ResourceOpenMenu resource={{ kind: "webpage", url: destination }} /> : null}
      {children}
    </div>
    {invalidAddress ? <p className="mt-2 px-1 text-xs text-danger" role="alert">{t("invalidAddress")}</p> : null}
  </div>;
}

/** Opaque sandbox: no product origin, top navigation, credentials or Desktop bridge. */
export function BrowserFrame({ url, title }: { url: string; title: string }) {
  return <iframe className="min-h-0 flex-1 border-0 bg-white" referrerPolicy="no-referrer" sandbox="allow-scripts allow-forms" src={url} title={title} />;
}
