// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { BrowserSnapshot } from "@talos/sdk-browser";
import {ComputerBrowserConfiguration} from "./ComputerBrowserConfiguration";
import {useAppStore} from "@/shared/store/app-store";
const mocks = vi.hoisted(() => ({ operate: vi.fn(), localId:"worker" as string|null,desktop:true }));
vi.mock("@/features/computers", async () => ({
  useBrowser:(await import("@/features/computers/browser-query")).useBrowser, useSelectedComputerId: () => "worker",useComputers:()=>({localId:mocks.localId}),
  useProductScope:()=>"alice",
}));
vi.mock("@/features/computers/hooks",()=>({useProductScope:()=>"alice"}));
vi.mock("@/shared/api/desktop-client",()=>({isDesktopRuntime:()=>mocks.desktop,getDesktopBridge:()=>null}));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ browser: { operate: mocks.operate } }) }));
afterEach(() => { cleanup();useAppStore.setState({workspaceWindows:[],workspaceActiveKey:null,workspaceOpen:false,workspaceExpanded:false}); vi.resetAllMocks();mocks.localId="worker";mocks.desktop=true; });
function snapshot(): BrowserSnapshot {
  return { enabled: true, host: "desktop", selectedTargetId: "managed", targets: [
    { targetId: "managed", displayName: "Talos browser", kind: "managed", readiness: { status: "ready" }, localOpen: "desktopOwned" },
    { targetId: "connected", displayName: "My Chrome", kind: "connected", readiness: { status: "needsConnection" }, localOpen: "externalUserWindow" },
  ], display: { kind: "unavailable", reason: "nativeHost" }, control: { generation: "generation", revision: 3, epoch: 1, owner: { kind: "idle" } } };
}
function setup(value: BrowserSnapshot) {
  mocks.operate.mockResolvedValue(value);
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={query}><ComputerBrowserConfiguration nodeId="worker"/></QueryClientProvider>);
  return query;
}
it("preserves both Desktop browser choices and sends a revision-bound target change", async () => {
  const query = setup(snapshot());
  await screen.findByText("Uses Desktop’s bundled browser. Opens locally without installing Chrome.");
  expect(screen.getAllByRole("radio")).toHaveLength(2);
  fireEvent.click(screen.getAllByRole("radio")[1]);
  await waitFor(() => expect(mocks.operate).toHaveBeenCalledWith({ operation: { kind: "selectTarget", targetId: "connected", expectedRevision: 3 } }));
  expect(screen.queryByRole("button", { name: /remote desktop/i })).not.toBeInTheDocument();
  query.clear();
});
it("keeps native MCP configuration free of browser execution controls",async()=>{
 const query=setup(snapshot());await screen.findByText("Talos browser");
 for(const name of ["Open browser","View computer","Pause automation","Resume automation","Take control"])
   expect(screen.queryByRole("button",{name})).not.toBeInTheDocument();
 query.clear();
});
it("keeps disabled Browser settings readable without admitting mutations", async () => {
  const value = snapshot(); value.enabled = false;
  const query = setup(value);
  await screen.findByText("Browser is disabled. Enable it in the MCP list to continue.");
  expect(screen.getAllByRole("radio")[0]).toBeDisabled();
  query.clear();
});

it("does not offer local window focus from Web or a different Desktop",async()=>{
  mocks.desktop=false;const query=setup(snapshot());await screen.findByText("Uses Desktop’s bundled browser. Opens locally without installing Chrome.");
  expect(screen.queryByRole("button",{name:"Open browser"})).not.toBeInTheDocument();query.clear();
});
it("keeps Docker MCP configuration separate from the remote viewer",async()=>{
  const value=snapshot();value.host="docker";value.display={kind:"dockerDesktop",targetId:"managed"};value.targets=value.targets.slice(0,1);value.targets[0].localOpen="unavailable";
  const query=setup(value);await screen.findByText("Talos browser");
  expect(screen.queryByRole("button",{name:"View computer"})).not.toBeInTheDocument();
  expect(screen.queryByRole("button",{name:"Pause automation"})).not.toBeInTheDocument();
  expect(useAppStore.getState().workspaceWindows).toHaveLength(0);query.clear();
});
