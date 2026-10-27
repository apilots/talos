import { selectionClasses } from "@/shared/ui/controlStyles";
import { useEffect, useLayoutEffect, useRef, type RefObject, type KeyboardEvent, type CSSProperties } from "react";
import "./RightWorkspace.css";
import { FileText, Globe2, Monitor, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ComputerDesktop, useComputer, useProductScope } from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { cn } from "@/shared/lib/cn";
import { AppResizeHandle } from "./AppResizeHandle";
import { WORKSPACE_WIDTH } from "@/shared/store/ui-slice";
import { AppTopBar, WorkspaceControls } from "./AppTopBar";
import { MarkdownFilePreview, useFileMetadata } from "@/features/files";
import { DesktopBrowserPage, desktopBrowserRequest, OpenWebpageButton, WebPagePreview } from "@/features/web-preview";
import { workspaceWindowKey, type WorkspaceWindow } from "@/shared/store/workspace-window";

type TabProps = {
  selected: boolean; selectedRef: RefObject<HTMLButtonElement | null>;
  id: string; onSelect: () => void; onClose: () => void; onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => void;
};
function WindowTab({ name, kind, selected, selectedRef, id, onSelect, onClose, onKeyDown }: TabProps & { name: string; kind: WorkspaceWindow["kind"] }) {
  const { t } = useTranslation("common");
  const Icon = kind === "computer" ? Monitor : kind === "markdown" ? FileText : Globe2;
  return <div className={cn("flex min-w-0 max-w-64 shrink-0 items-center rounded-lg border", selected ? `${selectionClasses.selected} ${selectionClasses.border}` : `border-transparent ${selectionClasses.idle}`)}>
    <button ref={selected ? selectedRef : undefined} id={id} role="tab" type="button" aria-selected={selected} tabIndex={selected ? 0 : -1} aria-controls={`${id}-panel`} onKeyDown={onKeyDown} onClick={onSelect} title={name} className="flex min-w-0 items-center gap-2 rounded-lg px-3 py-2 text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand">
      <Icon aria-hidden="true" size={13} className="shrink-0" /><span className="truncate">{name}</span>
    </button>
    <button type="button" aria-label={t("workspace.close", { name })} onClick={onClose} className="mr-1 flex size-6 shrink-0 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand"><X aria-hidden="true" size={12} /></button>
  </div>;
}
function ComputerTab({ nodeId, ...props }: TabProps & { nodeId: string }) {
  const computer = useComputer(nodeId);
  return <WindowTab {...props} kind="computer" name={computer.data?.displayName || `Computer · ${nodeId.slice(0, 8)}`} />;
}
function MarkdownTab({ fileId, scope, ...props }: TabProps & { fileId: string; scope: string }) {
  const file = useFileMetadata(scope, fileId);
  return <WindowTab {...props} kind="markdown" name={file.data?.originalFilename || `Markdown · ${fileId.slice(0, 8)}`} />;
}
function WorkspaceTab({ window, scope, ...props }: TabProps & { window: WorkspaceWindow; scope: string }) {
  const { t } = useTranslation("webPreview");
  switch (window.kind) {
    case "browser": return <WindowTab {...props} kind="browser" name={window.title} />;
    case "computer": return <ComputerTab {...props} nodeId={window.nodeId} />;
    case "markdown": return <MarkdownTab {...props} fileId={window.fileId} scope={scope} />;
    case "webpage": return <WindowTab {...props} kind="webpage" name={window.url ? new URL(window.url).host : t("newTab")} />;
  }
}

const tabId = (key: string) => `talos-workspace-tab-${encodeURIComponent(key)}`;

/** Webpage instances stay mounted; Computer media belongs only to the visible active tab. */
export function RightWorkspace({ modal, width, maxWidth, toggleRef }: {
  modal: boolean; width: number; maxWidth: number; toggleRef: RefObject<HTMLButtonElement | null>;
}) {
  const { t } = useTranslation("common");
  const scope = useProductScope();
  const windows = useAppStore(state => state.workspaceWindows);
  const active = useAppStore(state => state.workspaceActiveKey);
  const expanded = useAppStore(state => state.workspaceExpanded);
  const select = useAppStore(state => state.selectWorkspaceWindow);
  const close = useAppStore(state => state.closeWorkspaceWindow);
  const navigateWebpage = useAppStore(state => state.navigateWorkspaceWebpage);
  const visible = useAppStore(state => state.workspaceOpen);
  const focusRevision = useAppStore(state => state.workspaceFocusRevision);
  const previousFocusRevision = useRef(focusRevision);
  const setWidth = useAppStore(state => state.setWorkspaceWidth);
  const setResizing = useAppStore(state => state.setResizingPane);
  const setOpen = useAppStore(state => state.setWorkspaceOpen);
  const resizing = useAppStore(state => state.resizingPane !== null);
  const dialog = useRef<HTMLDialogElement>(null);
  const selectedTab = useRef<HTMLButtonElement>(null);
  const focusedSurface = useRef<Element | null>(null);
  useLayoutEffect(() => {
    const focused = focusedSurface.current;
    if (!focused || focused.isConnected) return;
    focusedSurface.current = null;
    if (visible && document.activeElement === document.body) selectedTab.current?.focus();
  }, [windows, visible]);
  const previous = useRef({ visible: false, modal });
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel = dialog.current;
    return () => {
      if (panel?.isConnected) return;
      if (panel?.open) panel.close();
      const focused = document.activeElement;
      if (focused instanceof HTMLElement && focused !== document.body && !panel?.contains(focused)) return;
      if (toggleRef.current && !toggleRef.current.disabled) toggleRef.current.focus();
      else if (opener?.isConnected && !panel?.contains(opener)) opener.focus();
    };
  }, [toggleRef]);
  useEffect(() => {
    const panel = dialog.current;
    if (!panel) return;
    const opening = visible && !previous.current.visible;
    const changingMode = visible && previous.current.visible && previous.current.modal !== modal;
    const closing = !visible && previous.current.visible;
    previous.current = { visible, modal: visible ? modal : previous.current.modal };
    if (!opening && !changingMode && !closing) return;
    const focus = document.activeElement;
    const inside = focus instanceof HTMLElement && panel.contains(focus);
    const restoreFocus = focus === document.body || inside;
    // Do not make a live iframe non-rendered between the two native presentations.
    const display = panel.style.display;
    if (changingMode) panel.style.display = "flex";
    try {
      if (panel.open) panel.close();
      if (visible) {
        if (modal) panel.showModal(); else panel.open = true;
        if (changingMode && inside) {
          if (focus.isConnected && !focus.closest("[hidden], [inert]")) {
            // Native modal entry may clear a cross-origin frame's inner input.
            // Restore its browsing context without reading its protected DOM.
            if (focus instanceof HTMLIFrameElement) focus.contentWindow?.focus();
            else focus.focus();
          } else selectedTab.current?.focus();
        } else if (opening || modal) selectedTab.current?.focus();
        else if (focus instanceof HTMLElement && focus !== document.body && focus.isConnected) focus.focus();
      } else if (restoreFocus) toggleRef.current?.focus();
    } finally {
      if (changingMode) panel.style.display = display;
    }
    // Unmount cleanup lives above. Closing during dependency cleanup would let
    // the native dialog restore its opener before this update observes focus.
  }, [modal, visible, toggleRef]);
  useEffect(() => {
    if (previousFocusRevision.current === focusRevision) return;
    previousFocusRevision.current = focusRevision;
    if (!visible) return;
    selectedTab.current?.focus();selectedTab.current?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [focusRevision, visible]);
  const modalPresentation = visible ? modal : previous.current.modal;
  return <dialog onFocusCapture={event => { focusedSurface.current = event.target; }} onBlurCapture={() => { focusedSurface.current = null; }} style={{ "--workspace-width": `${width}px` } as CSSProperties} data-presentation={modalPresentation ? "modal" : "split"} data-resizing={resizing} inert={!visible} aria-hidden={!visible} id="talos-right-workspace" ref={dialog} aria-label={t("workspace.title")} onCancel={event => { event.preventDefault(); setOpen(false); }} className={cn(
    "m-0 min-h-0 max-h-none max-w-none overflow-visible flex-col border-0 border-l border-divider bg-surface p-0 text-ink open:flex backdrop:bg-transparent",
    modalPresentation ? "fixed inset-y-0 left-14 right-0 z-40 h-screen w-[calc(100vw-56px)]" : "relative h-full shrink-0",
  )}>
    {!modal && visible ? <AppResizeHandle label={t("workspace.resize")} controls="talos-right-workspace" value={width} min={WORKSPACE_WIDTH.min} max={maxWidth} direction={-1} onResize={setWidth} onDragging={dragging => setResizing(dragging ? "workspace" : null)} className="-left-1 inset-y-0" /> : null}
    <AppTopBar label={t("workspace.controls")} className="relative gap-1.5 px-3 sm:px-3">
      <div role="tablist" aria-label={t("workspace.windows")} className="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto">
      {windows.map(window => {
        const key = workspaceWindowKey(window);
        return <WorkspaceTab key={key} window={window} scope={scope} selected={active === key} selectedRef={selectedTab} id={tabId(key)} onSelect={() => select(key)} onKeyDown={event => {
          const index = windows.findIndex(entry => workspaceWindowKey(entry) === key);
          const next = event.key === "ArrowRight" ? (index + 1) % windows.length
            : event.key === "ArrowLeft" ? (index + windows.length - 1) % windows.length
            : event.key === "Home" ? 0 : event.key === "End" ? windows.length - 1 : null;
          if (next !== null) { event.preventDefault(); select(workspaceWindowKey(windows[next])); }
        }} onClose={() => {
          if (window.kind === "browser") {
            const sessionId = useAppStore.getState().desktopBrowser?.sessionId;
            if (sessionId) void desktopBrowserRequest({ kind: "close", sessionId, tabId: window.id }).then(() => useAppStore.getState().focusWorkspace()).catch(error => console.error("Browser tab close failed", error));
            return;
          }
          close(key);
          if (active !== key) selectedTab.current?.focus();
        }} />;
      })}
      </div>
      <OpenWebpageButton icon="plus" />
      {visible ? <WorkspaceControls toggleRef={toggleRef} /> : null}
    </AppTopBar>
    <div className="min-h-0 flex-1 overflow-hidden bg-background">
      {windows.map(window => {
        const key=workspaceWindowKey(window);
        const activeWindow=key===active;
        const selected=visible && activeWindow;
        return <div key={key} id={`${tabId(key)}-panel`} role="tabpanel" aria-labelledby={tabId(key)} hidden={!activeWindow} inert={!selected} className="h-full min-h-0">
          {window.kind === "browser" ? activeWindow ? <DesktopBrowserPage tab={window} selected={selected} /> : null
            : window.kind === "webpage" ? <WebPagePreview url={window.url} onNavigate={url => navigateWebpage(key,url)} />
            : selected && window.kind === "computer" ? <ComputerDesktop nodeId={window.nodeId} expanded={expanded} onExitInput={() => toggleRef.current?.focus()} />
            : activeWindow && window.kind === "markdown" ? <MarkdownFilePreview fileId={window.fileId} connectionKey={scope} /> : null}
        </div>;
      })}
    </div>
  </dialog>;
}
