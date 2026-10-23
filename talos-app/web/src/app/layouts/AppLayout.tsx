import { useEffect, useRef, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { ComputerRail, useComputers, useSelectedComputerId, useProductScope } from "@/features/computers";
import { SidebarContainer } from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { AppNavigation, AppSessionContext, AppTopBar, WorkspaceControls } from "./AppTopBar";
import { ComputerHomePage } from "@/pages/home/ComputerHomePage";
import { RightWorkspace } from "./RightWorkspace";
import { AppSidebar } from "./AppSidebar";
import { AppMainContent } from "./AppMainContent";
import { AppBusinessRegion } from "./AppBusinessRegion";
import { layoutWidths } from "./layout-sizing";
import { SidebarComputerEntry } from "./SidebarComputerEntry";

/** Five areas: app navigation, rail, project/sidebar, route content, workspace. */
export function AppLayout() {
  const location = useLocation();
  const { selected: computer } = useComputers();
  const computerId = useSelectedComputerId();
  const scope = useProductScope();
  const sidebarOpen = useAppStore(state => state.sidebarOpen);
  const setSidebarOpen = useAppStore(state => state.setSidebarOpen);
  const workspaceOpen = useAppStore(state => state.workspaceOpen);
  const workspaceExpanded = useAppStore(state => state.workspaceExpanded);
  const windows = useAppStore(state => state.workspaceWindows);
  const sidebarWidth = useAppStore(state => state.sidebarWidth);
  const workspaceWidth = useAppStore(state => state.workspaceWidth);
  const setWorkspaceWidth = useAppStore(state => state.setWorkspaceWidth);
  const resizing = useAppStore(state => state.resizingPane !== null);
  const [viewport, setViewport] = useState(() => ({ wide: window.matchMedia("(min-width: 1024px)").matches, width: window.innerWidth }));
  const [sidebarDrawerOpen, setSidebarDrawerOpen] = useState(false);
  const { wide } = viewport;
  const standalonePage = location.pathname === "/computers" || location.pathname.startsWith("/settings");
  const choosingComputer = !computerId && !standalonePage && !location.pathname.startsWith("/sessions/");
  const sidebarAvailable = !choosingComputer && !standalonePage;
  const showSidebar = sidebarAvailable && (wide ? sidebarOpen : sidebarDrawerOpen);
  const workspaceVisible = workspaceOpen && windows.length > 0;
  const workspaceModal = workspaceVisible && (workspaceExpanded || !wide);
  const sidebarVisible = showSidebar && !workspaceModal;
  const sidebarModal = sidebarVisible && !wide;
  useEffect(() => {
    // A narrow modal is a transient user action, not the persisted wide layout.
    if (wide || workspaceModal || !sidebarAvailable) setSidebarDrawerOpen(false);
  }, [wide, workspaceModal, sidebarAvailable]);
  const sidebarToggle = useRef<HTMLButtonElement>(null);
  const workspaceToggle = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const viewport = window.matchMedia("(min-width: 1024px)");
    const update = () => setViewport({ wide: viewport.matches, width: window.innerWidth });
    update(); viewport.addEventListener("change", update);window.addEventListener("resize", update);
    return () => { viewport.removeEventListener("change", update);window.removeEventListener("resize", update); };
  }, []);
  const sizes = layoutWidths(viewport.width - 56, wide && sidebarVisible, workspaceVisible && !workspaceModal, sidebarWidth, workspaceWidth);
  useEffect(() => {
    // Retain the admitted width so closing the sidebar does not snap the outer boundary.
    if (workspaceVisible && !workspaceModal && workspaceWidth !== Math.round(sizes.workspace)) setWorkspaceWidth(sizes.workspace);
  }, [workspaceVisible, workspaceModal, workspaceWidth, sizes.workspace, setWorkspaceWidth]);
  const navigation = <AppNavigation sidebarVisible={sidebarVisible} sidebarAvailable={sidebarAvailable} sidebarToggleRef={sidebarToggle} onToggleSidebar={() => wide ? setSidebarOpen(!sidebarOpen) : setSidebarDrawerOpen(open => !open)} />;
  const context = <AppSessionContext />;
  const sidebarExpanded = wide && sidebarVisible;
  const mainHeader = <AppTopBar className="h-full border-b-0 px-6 sm:px-6">{context}{!workspaceVisible ? <WorkspaceControls toggleRef={workspaceToggle} /> : null}</AppTopBar>;
  const modalHeader = <AppTopBar>{navigation}{context}{!workspaceVisible ? <WorkspaceControls toggleRef={workspaceToggle} /> : null}</AppTopBar>;
  const sidebarRegion = (wide && sidebarAvailable) || sidebarModal ? <AppSidebar modal={sidebarModal} visible={sidebarVisible} width={wide ? sizes.sidebar : Math.min(sidebarWidth, viewport.width - 56)} resizing={resizing} toggleRef={sidebarToggle} onClose={() => wide ? setSidebarOpen(false) : setSidebarDrawerOpen(false)} topBar={sidebarModal ? modalHeader : undefined}><SidebarContainer computer={computer} computerEntry={<SidebarComputerEntry />} /></AppSidebar> : null;
  return <div className="flex h-screen overflow-hidden bg-surface text-ink">
    <div className="h-full w-14 shrink-0"><ComputerRail /></div>
    <AppBusinessRegion inactive={workspaceModal} headerVisible={!choosingComputer} sidebarExpanded={sidebarExpanded} sidebarWidth={sizes.sidebar} sidebarMax={sizes.sidebarMax} navigation={!sidebarModal ? navigation : undefined} mainHeader={!sidebarModal ? mainHeader : null} sidebar={wide ? sidebarRegion : null}>
      <AppMainContent key={`${scope}:${location.pathname.startsWith("/sessions/") ? "session" : computerId ?? "pending"}`}>
        {choosingComputer ? <ComputerHomePage /> : <Outlet />}
      </AppMainContent>
    </AppBusinessRegion>
    {sidebarModal ? sidebarRegion : null}
    {windows.length > 0 ? <RightWorkspace modal={workspaceModal} width={sizes.workspace} maxWidth={sizes.workspaceMax} toggleRef={workspaceToggle} /> : null}
  </div>;
}
