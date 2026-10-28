import type { FilePreviewDescriptor } from "@talos/sdk-core";
import type { PreviewOpenTarget, PreviewOpenResource } from "@talos/sdk-app-server/desktop";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ExternalLink, Link2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { openResource, resourceReference, type PreviewResource } from "@/shared/lib/open-resource";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/shared/ui/DropdownMenu";
import { useAppStore } from "@/shared/store/app-store";

/** Native target discovery stays in the shell; this menu only consumes capabilities. */
export function ResourceOpenMenu({ resource, preview: prepared }: { resource: PreviewResource; preview?: FilePreviewDescriptor }) {
  const { t } = useTranslation("webPreview");
  const trigger = useRef<HTMLButtonElement>(null);
  const [container, setContainer] = useState<HTMLElement | null>(null);
  const [failed, setFailed] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [targets, setTargets] = useState<PreviewOpenTarget[]>([]);
  const native = getDesktopBridge()?.preview;
  const webpageUrl = resource.kind === "webpage" ? resource.url : null;
  const previewId = prepared?.previewId;
  const resourcePath = prepared?.entryPath;
  const nativeResource = useMemo<PreviewOpenResource | undefined>(() => webpageUrl !== null ? { kind: "webpage", url: webpageUrl }
    : previewId && resourcePath ? { kind: "file", previewId, resourcePath } : undefined, [webpageUrl, previewId, resourcePath]);
  useEffect(() => {
    setTargets([]);
    if (!menuOpen || !native || !nativeResource) return;
    let active = true;
    void native.targets(nativeResource).then(targets => { if (active) setTargets(targets); }, () => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [menuOpen, native, nativeResource]);
  const owner = useAppStore(state => state.workspaceOwner);
  const active = useAppStore(state => state.workspaceActiveKey);
  const visible = useAppStore(state => state.workspaceOpen);
  useEffect(() => { setMenuOpen(false); }, [owner, active, visible]);
  const run = async (action: () => Promise<unknown>) => {
    setFailed(false);
    try { await action(); } catch { setFailed(true); }
  };
  const preview = () => run(() => openResource(resource.kind !== "webpage" ? resource : { ...resource, intent: "preview" }));
  return <div className="relative shrink-0">
    <div className="flex h-8 items-center rounded-lg border border-border bg-card text-xs text-ink">
      <button type="button" className="h-full rounded-l-lg px-2.5 hover:bg-hover focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand" onClick={preview}>Open in</button>
      <DropdownMenu modal={false} open={menuOpen} onOpenChange={open => { setMenuOpen(open); if (open) setContainer(trigger.current?.closest("dialog") ?? null); }}>
        <DropdownMenuTrigger asChild><button ref={trigger} type="button" aria-label={t("openOptions")} className="flex h-full w-6 items-center justify-center rounded-r-lg border-l border-divider bg-hover/40 text-muted hover:bg-hover focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand"><ChevronDown aria-hidden="true" size={13} /></button></DropdownMenuTrigger>
        <DropdownMenuContent container={container} align="end" className="min-w-44" onCloseAutoFocus={event => { if (trigger.current?.closest("[hidden], [inert]")) event.preventDefault(); }}>
          <DropdownMenuItem onSelect={preview}><img src={`${import.meta.env.BASE_URL}app-icon.svg`} alt="" className="size-4" />Talos</DropdownMenuItem>
          {native && nativeResource ? targets.map(target => <DropdownMenuItem key={target.id} onSelect={() => run(() => native.open({ resource: nativeResource, targetId: target.id }))}>
            {target.icon ? <img src={target.icon} alt="" className="size-4" /> : <ExternalLink aria-hidden="true" size={15} />}{target.id === "system" ? t(resource.kind === "webpage" ? "openExternal" : "openSystemApplication") : target.label}
          </DropdownMenuItem>) : resource.kind === "webpage" ? <DropdownMenuItem onSelect={() => run(() => openResource({ ...resource, intent: "external" }))}><ExternalLink aria-hidden="true" size={15} />{t("openExternal")}</DropdownMenuItem> : null}
          <DropdownMenuSeparator className="my-1 h-px bg-divider" />
          <DropdownMenuItem onSelect={() => run(() => navigator.clipboard.writeText(resourceReference(resource)))}><Link2 aria-hidden="true" size={15} />{t("copyLink")}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
    {failed ? <p role="alert" className="absolute right-0 top-full z-10 mt-2 w-56 rounded-lg border border-danger/25 bg-card p-3 text-xs text-danger shadow-panel">{t("actionFailed")}</p> : null}
  </div>;
}
