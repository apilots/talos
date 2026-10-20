import { useEffect, useRef, type CSSProperties, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/shared/store/app-store";
import { cn } from "@/shared/lib/cn";

/** Ordinary wide sidebar; native modal only when it overlays the narrow business area. */
export function AppSidebar({ modal, visible, width, resizing, topBar, toggleRef, children }: {
  modal: boolean; visible: boolean; width: number; resizing: boolean; topBar?: ReactNode; toggleRef: RefObject<HTMLButtonElement | null>; children: ReactNode;
}) {
  const { t } = useTranslation("common");
  const close = useAppStore(state => state.setSidebarOpen);
  const dialog = useRef<HTMLDialogElement>(null);
  const surface = useRef<HTMLElement | null>(null);
  const content = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const panel = dialog.current;
    const focused = document.activeElement;
    if (modal && visible && panel && !panel.open) panel.showModal();
    if (focused === document.body || (!visible && focused instanceof Node && surface.current?.contains(focused))) toggleRef.current?.focus();
  }, [modal, visible, toggleRef]);
  useEffect(() => {
    const panel = dialog.current;
    return () => {
      if (panel?.open) panel.close();
      if (modal) toggleRef.current?.focus();
    };
  }, [modal, toggleRef]);
  const style = { "--sidebar-open-width": `${width}px` } as CSSProperties;
  const drawer = <div ref={content} className={cn("min-h-0 w-[var(--sidebar-open-width)] flex-1 overflow-hidden bg-sidebar motion-reduce:transform-none max-lg:w-[min(var(--sidebar-open-width),calc(100vw-56px))]", resizing ? "" : "transition-[transform,opacity] duration-200 ease-out motion-reduce:transition-none", visible ? "translate-x-0 opacity-100" : "-translate-x-full opacity-0")}>{children}</div>;
  if (!modal) return <aside id="talos-sidebar" ref={surface} style={style} inert={!visible} aria-label={t("navigation.sidebar")} className="flex h-full min-h-0 w-[var(--sidebar-open-width)] shrink-0 flex-col overflow-hidden bg-sidebar">{drawer}</aside>;
  return <dialog style={style} id="talos-sidebar" ref={node => { dialog.current = node; surface.current = node; }} inert={!visible} aria-label={t("navigation.sidebar")} onCancel={event => { event.preventDefault(); close(false); }}
    onClick={event => {
      if (event.target !== event.currentTarget) return;
      const bounds = content.current?.getBoundingClientRect();
      if (bounds && (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom)) close(false);
    }} className="fixed inset-y-0 left-14 right-0 m-0 h-screen max-h-none w-[calc(100vw-56px)] max-w-none flex-col border-0 bg-transparent p-0 text-ink open:flex backdrop:bg-black/25">
    {topBar}{drawer}
  </dialog>;
}
