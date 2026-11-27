// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { StrictMode, type ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, useLocation } from "react-router-dom";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/shared/i18n/setup";
import { resetTalosClientFactory, setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { connectionScope } from "@/shared/store/connection-scope";
import { useAppStore } from "@/shared/store/app-store";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import { TaloLifecycleMenu } from "@/features/computers";
import { ComputerHomePage } from "./ComputerHomePage";

const desktop = vi.hoisted(() => ({ active: false }));
vi.mock("@/shared/api/desktop-client", () => ({ isDesktopRuntime: () => desktop.active, getDesktopBridge: () => desktop.active ? {} : undefined }));
vi.mock("@talos/sdk-app-server/desktop", async (importOriginal) => ({
  ...await importOriginal<typeof import("@talos/sdk-app-server/desktop")>(),
  createElectronAppServerClient: () => ({ desktop: { workerIdentity:  () => Promise.resolve(({ nodeId: "desktop-local", controlPlaneEndpoint: null, systemInfo: null })) } }),
}));
const list = vi.fn();
const syncStatus = vi.fn();
const readManagedWorker = vi.fn();
const createManagedWorker = vi.fn();
const startManagedWorker = vi.fn();
const deleteManagedWorker = vi.fn();
const worker = {
  instanceId: "talo-instance", nodeId: "talo-node", conversationSessionId: "sess_talo", generation: 1, phase: "waitingForRegistration",
  operation: "create", cpuCores: 4, memoryBytes: 8 * 1024 ** 3, diskQuota: "unsupported", error: null,
} satisfies AppProtocol.ManagedWorkerInfo;
const query = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });

function Location() { return <output aria-label="Location">{useLocation().pathname}</output>; }
function show(path = "/files", content: ReactNode = <ComputerHomePage />) {
  return render(<StrictMode><QueryClientProvider client={query}><MemoryRouter initialEntries={[path]}>{content}<Location /></MemoryRouter></QueryClientProvider></StrictMode>);
}

beforeEach(() => {
  desktop.active = false;
  syncStatus.mockReset().mockResolvedValue({ localComputerId: null });
  void i18n.changeLanguage("en");
  list.mockReset().mockResolvedValue({ data: topologyNodes, total: 2, nextCursor: null });
  readManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: null });
  createManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: worker });
  startManagedWorker.mockReset();
  deleteManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: null });
  useAppStore.setState({
    selectedComputers: {}, workspaceWindows: [], workspaceSnapshots: {}, workspaceOwner: null, workspaceOpen: false,
    connection: { ...useAppStore.getInitialState().connection, authStatus: "authenticated", accountType: "Max", userId: "alice" },
  });
  setTalosClientFactory(() => ({
    computers: {
      list, readManagedWorker, createManagedWorker, startManagedWorker, deleteManagedWorker,
      read: async ({ nodeId }: { nodeId: string }) => nodeId === worker.nodeId
        ? { ...topologyNodes[0], nodeId, displayName: "Talo", managedWorkerPhase: "ready" }
        : topologyNodes.find(node => node.nodeId === nodeId) ?? Promise.reject(new TypeError("Directory unavailable")),
    },
    browser: { operate:  () => Promise.resolve(({ enabled: false, host: "docker", selectedTargetId: null, targets: [], display: { kind: "dockerDesktop", targetId: "managed" }, control: { generation: "gen-1", revision: 1, epoch: 0, owner: { kind: "idle" } } })) },
    sync: { status: syncStatus },
  }) as unknown as TalosWebClient);
});
afterEach(() => { cleanup(); query.clear(); resetTalosClientFactory(); });

it("uses the existing computer rows and selects even an offline computer without changing its status", async () => {
  show();
  expect(await screen.findByRole("heading", { name: "Choose a computer" })).toBeVisible();
  expect(await screen.findByRole("button", { name: "Studio · Connected" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Build Worker · Offline" }));
  expect(useAppStore.getState().selectedComputers[connectionScope(useAppStore.getState().connection)]).toBe("worker-b");
  expect(screen.getByLabelText("Location")).toHaveTextContent("/");
});

it.each(["starting", "restarting", "stopped", "error", "ready"] as const)("shows current Desktop runtime %s in the home picker despite stale directory availability", async (kind) => {
  desktop.active = true;
  list.mockResolvedValue({ data: [{ ...topologyNodes[0], nodeId: "desktop-local", available: kind !== "ready", status: "disabled" }], total: 1, nextCursor: null });
  useAppStore.getState().setConnection({ workerRuntimeState: { kind, message: null } });
  show();
  const label = `Studio · ${i18n.t("computers:local")} · ${i18n.t(`computers:localRuntime.${kind}`)} · ${i18n.t("computers:remoteDisabled")}`;
  expect(await screen.findByRole("button", { name: label })).toBeVisible();
  act(() => useAppStore.getState().setConnection({ workerRuntimeState: { kind: "restarting", message: null } }));
  expect(screen.getByRole("button", { name: `Studio · ${i18n.t("computers:local")} · ${i18n.t("computers:localRuntime.restarting")} · ${i18n.t("computers:remoteDisabled")}` })).toBeVisible();
});

it("keeps Web sync-local computers under Server status authority", async () => {
  syncStatus.mockResolvedValue({ localComputerId: topologyNodes[0].nodeId });
  useAppStore.getState().setConnection({ workerRuntimeState: { kind: "restarting", message: null } });
  show();
  await waitFor(() => expect(useAppStore.getState().selectedComputers[connectionScope(useAppStore.getState().connection)]).toBe(topologyNodes[0].nodeId));
  expect(await screen.findByRole("button", { name: "Studio · Connected" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /This computer/ })).not.toBeInTheDocument();
});

it("shows connection help inline and switches the two chooser tabs by keyboard", async () => {
  list.mockResolvedValue({ data: [], total: 0, nextCursor: null });
  show();
  expect(await screen.findByRole("heading", { name: "Connect a personal computer" })).toBeVisible();
  expect(screen.getByText(/then sign in with the same account/)).toBeVisible();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(document.body).toHaveFocus();
  const personal = screen.getByRole("tab", { name: "Personal computers" });
  personal.focus();
  fireEvent.keyDown(personal, { key: "ArrowRight" });
  const talo = screen.getByRole("tab", { name: "Talo" });
  expect(talo).toHaveFocus();
  expect(talo).toHaveAttribute("aria-selected", "true");
  expect(await screen.findByRole("button", { name: "Initialize Talo" })).toBeVisible();
  fireEvent.keyDown(talo, { key: "Home" });
  expect(personal).toHaveFocus();
  expect(screen.getByText(/then sign in with the same account/)).toBeVisible();
});

it("does not confuse directory loading or failure with zero registered computers and supports retry", async () => {
  let reject!: (reason: Error) => void;
  list.mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail; }));
  show();
  expect(within(screen.getByRole("region", { name: "Home" })).getByRole("status")).toHaveTextContent("Loading");
  expect(screen.queryByRole("heading", { name: "Connect a personal computer" })).not.toBeInTheDocument();
  await waitFor(() => expect(list).toHaveBeenCalled());
  await act(() => Promise.resolve(reject(new TalosApiError({ code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Directory unavailable.", localization: null }))));
  expect(await screen.findByRole("alert")).toHaveTextContent(i18n.t("computers:networkError"));
  expect(screen.getByRole("alert")).toHaveClass("text-danger");
  expect(screen.queryByRole("heading", { name: "Connect a personal computer" })).not.toBeInTheDocument();
  list.mockResolvedValue({ data: [], total: 0, nextCursor: null });
  fireEvent.click(screen.getByRole("button", { name: "Retry now" }));
  expect(await screen.findByRole("heading", { name: "Connect a personal computer" })).toBeVisible();
});

it.each([false, true])("hides cached remotes after directory failure while preserving the local Desktop (Desktop=%s)", async (native) => {
  desktop.active = native;
  useAppStore.getState().setConnection({ workerRuntimeState: { kind: "ready", message: null } });
  show();
  await screen.findByRole("button", { name: "Studio · Connected" });
  const scope = connectionScope(useAppStore.getState().connection);
  list.mockRejectedValue(new TypeError("Offline"));
  await act(async () => { await query.refetchQueries({ queryKey: ["talos", scope, "computers"] }); });
  expect(await screen.findByRole("alert")).toHaveTextContent(i18n.t("computers:networkError"));
  expect(screen.queryByRole("button", { name: /Studio|Build Worker/ })).not.toBeInTheDocument();
  expect(useAppStore.getState().selectedComputers[scope]).not.toBe(topologyNodes[0].nodeId);
  if (native) {
    const local = screen.getByRole("button", { name: "This computer · This computer · Ready" });
    expect(local).toBeEnabled();
    fireEvent.click(local);
    expect(useAppStore.getState().selectedComputers[scope]).toBe("desktop-local");
  }
  list.mockResolvedValue({ data: topologyNodes, total: 2, nextCursor: null });
  fireEvent.click(screen.getByRole("button", { name: i18n.t("common:retryNow") }));
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  expect(screen.getByRole("button", { name: "Studio · Connected" })).toBeEnabled();
});

it.each([false, true])("keeps this Desktop selectable when an empty cloud directory fails (cached=%s)", async (cached) => {
  desktop.active = true;
  useAppStore.getState().setConnection({ workerRuntimeState: { kind: "starting", message: null } });
  list.mockImplementation(() => cached ? Promise.resolve({ data: [], total: 0, nextCursor: null }) : Promise.reject(new TypeError("Offline")));
  show();
  if (cached) {
    await screen.findByRole("button", { name: /This computer.*Starting/ });
    list.mockRejectedValue(new TypeError("Offline"));
    await act(async () => { await query.refetchQueries({ queryKey: ["talos", connectionScope(useAppStore.getState().connection), "computers"] }); });
  }
  expect(await screen.findByRole("alert")).toHaveTextContent(i18n.t("computers:networkError"));
  const local = await screen.findByRole("button", { name: /This computer · This computer · Starting…/ });
  expect(screen.queryByRole("button", { name: "Studio · Connected" })).not.toBeInTheDocument();
  fireEvent.click(local);
  expect(useAppStore.getState().selectedComputers[connectionScope(useAppStore.getState().connection)]).toBe("desktop-local");
});

it("searches beyond the initial computer list without treating no matches as an unregistered account", async () => {
  const more = { ...topologyNodes[0], nodeId: "remote", displayName: "Remote office" };
  list.mockImplementation(({ query: search }) => Promise.resolve(search
    ? { data: search === "office" ? [more] : [], total: search === "office" ? 1 : 0, nextCursor: null }
    : { data: topologyNodes, total: 21, nextCursor: "next" }));
  show();
  const search = await screen.findByRole("searchbox", { name: "Search personal computers" });
  expect(screen.queryByRole("button", { name: "Show more" })).not.toBeInTheDocument();
  fireEvent.change(search, { target: { value: "office" } });
  await waitFor(() => expect(list).toHaveBeenCalledWith(expect.objectContaining({ query: "office" }), expect.any(AbortSignal)));
  expect(await screen.findByRole("button", { name: "Remote office · Connected" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Studio · Connected" })).not.toBeInTheDocument();
  fireEvent.change(search, { target: { value: "no-match" } });
  expect(await screen.findByText("No personal computers found")).toBeVisible();
  expect(screen.getByRole("heading", { name: "Choose a computer" })).toBeVisible();
  expect(screen.queryByRole("heading", { name: "Connect a personal computer" })).not.toBeInTheDocument();
});

it("translates the first-computer prompt and connection guidance", async () => {
  await i18n.changeLanguage("zh-CN");
  list.mockResolvedValue({ data: [], total: 0, nextCursor: null });
  show();
  expect(await screen.findByRole("heading", { name: "添加个人电脑" })).toBeVisible();
  expect(screen.getByRole("tab", { name: "个人电脑" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByText(/然后登录同一账号/)).toBeVisible();
});

it("keeps Talo out of the personal directory and resumes setup after leaving or reloading the tab", async () => {
  const node = { ...topologyNodes[0], nodeId: worker.nodeId, displayName: "Talo", managedWorkerPhase: worker.phase };
  list.mockResolvedValue({ data: [node], total: 1, nextCursor: null });
  const view = show();
  expect(await screen.findByRole("heading", { name: "Connect a personal computer" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /^Talo ·/ })).not.toBeInTheDocument();
  expect(Object.values(useAppStore.getState().selectedComputers)).not.toContain(worker.nodeId);

  fireEvent.click(screen.getByRole("tab", { name: "Talo" }));
  const init = await screen.findByRole("button", { name: "Initialize Talo" });
  readManagedWorker.mockResolvedValue({ enabled: true, instance: worker });
  fireEvent.click(init);
  const activeStep = await screen.findByText("Wait for Worker registration");
  expect(activeStep).toHaveAttribute("aria-current", "step");
  expect(screen.queryByText(/does not enforce a disk quota/)).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("tab", { name: "Personal computers" }));
  fireEvent.click(screen.getByRole("tab", { name: "Talo" }));
  expect(await screen.findByText("Wait for Worker registration")).toBeVisible();
  view.unmount();
  query.clear();
  show("/talo");
  expect(await screen.findByText("Wait for Worker registration")).toBeVisible();
  expect(createManagedWorker).toHaveBeenCalledTimes(1);
});

it("connects the ready Talo in a desktop modal without adding a RightWorkspace tab", async () => {
  readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "ready", operation: null } });
  show("/talo");
  const connect = await screen.findByRole("button", { name: "Connect desktop" });
  expect(connect.parentElement).toHaveTextContent("This runtime does not enforce a disk quota.");
  fireEvent.click(connect);
  expect(await screen.findByRole("dialog", { name: "Talo desktop" })).toBeVisible();
  expect(useAppStore.getState().workspaceWindows).toHaveLength(0);
  expect(startManagedWorker).not.toHaveBeenCalled();
});

it("observes Talo going offline and reconnecting while the page stays open", async () => {
  vi.useFakeTimers();
  try {
    const ready = { ...worker, phase: "ready", operation: null };
    readManagedWorker.mockResolvedValue({ enabled: true, instance: ready });
    show("/talo");
    await act(async () => { await vi.advanceTimersByTimeAsync(25); });
    expect(screen.getByRole("heading", { name: "Talo is ready" })).toBeVisible();

    readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...ready, phase: "offline" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(5_025); });
    expect(screen.getByRole("heading", { name: "Talo is offline" })).toBeVisible();

    readManagedWorker.mockResolvedValue({ enabled: true, instance: ready });
    await act(async () => { await vi.advanceTimersByTimeAsync(5_025); });
    expect(screen.getByRole("heading", { name: "Talo is ready" })).toBeVisible();
  } finally {
    vi.useRealTimers();
  }
});

it("retries an existing failed Talo instead of initializing another instance", async () => {
  const error = { code: AppProtocol.ErrorCode.Timeout, message: "The Worker did not register.", localization: null };
  readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "failed", error } });
  startManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "ready", operation: null, diskQuota: "enforced" } });
  show("/talo");
  expect(await screen.findByRole("alert")).toHaveTextContent(error.message);
  fireEvent.click(screen.getByRole("button", { name: "Retry startup" }));
  expect(await screen.findByRole("button", { name: "Connect desktop" })).toBeVisible();
  expect(screen.queryByText(/does not enforce a disk quota/)).not.toBeInTheDocument();
  expect(startManagedWorker).toHaveBeenCalledWith({ instanceId: worker.instanceId, expectedGeneration: worker.generation });
  expect(createManagedWorker).not.toHaveBeenCalled();
});

it("wakes Talo on an explicit desktop request when it has gone to sleep since the last read", async () => {
  readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "ready", operation: null } });
  startManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "ready", operation: null } });
  show("/talo");
  const connect = await screen.findByRole("button", { name: "Connect desktop" });
  expect(startManagedWorker).not.toHaveBeenCalled();
  expect(useAppStore.getState().workspaceWindows).toHaveLength(0);
  readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "sleeping", operation: null } });
  fireEvent.click(connect);
  expect(await screen.findByRole("dialog", { name: "Talo desktop" })).toBeVisible();
  expect(useAppStore.getState().workspaceWindows).toHaveLength(0);
  expect(startManagedWorker).toHaveBeenCalledExactlyOnceWith({ instanceId: worker.instanceId, expectedGeneration: worker.generation });
});

it("requires explicit confirmation for Talo deletion", async () => {
  readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "stopped", operation: null } });
  show("/talo", <TaloLifecycleMenu desktopNodeId={worker.nodeId} />);
  const menu = await screen.findByRole("button", { name: "Manage Talo" });
  fireEvent.keyDown(menu, { key: "Enter" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Delete Talo" }));
  const dialog = await screen.findByRole("alertdialog", { name: "Delete Talo?" });
  expect(dialog).toHaveTextContent(/will be permanently deleted/);
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(deleteManagedWorker).not.toHaveBeenCalled();
  await waitFor(() => expect(menu).toHaveFocus());
  fireEvent.keyDown(menu, { key: "Enter" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Delete Talo" }));
  fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Delete Talo" }));
  await waitFor(() => expect(deleteManagedWorker).toHaveBeenCalledExactlyOnceWith({
    instanceId: worker.instanceId, expectedGeneration: worker.generation, confirmDataLoss: true,
  }));
});

it.each([
  ["user", "Free"], ["user", "Plus"], ["user", "Pro"],
] as const)("hides Talo and avoids lifecycle calls for %s %s", async (authRole, accountType) => {
  useAppStore.getState().setConnection({ authRole, accountType });
  show("/talo");
  expect(await screen.findByRole("heading", { name: "Choose a computer" })).toBeVisible();
  expect(screen.queryByRole("tab", { name: "Talo" })).not.toBeInTheDocument();
  expect(screen.queryByText(/your AI assistant/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Initialize Talo" })).not.toBeInTheDocument();
  expect(readManagedWorker).not.toHaveBeenCalled();
  expect(createManagedWorker).not.toHaveBeenCalled();
});

it("allows an administrator with a Free type and hides a retained desktop panel after downgrade", async () => {
  useAppStore.getState().setConnection({ authRole: "admin", accountType: "Free" });
  readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "ready", operation: null } });
  show("/talo", <TaloLifecycleMenu desktopNodeId={worker.nodeId} />);
  expect(await screen.findByRole("button", { name: "Manage Talo" })).toBeVisible();
  act(() => useAppStore.getState().setConnection({ authRole: "user" }));
  expect(screen.queryByRole("button", { name: "Manage Talo" })).not.toBeInTheDocument();
});
