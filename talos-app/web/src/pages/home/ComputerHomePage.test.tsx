// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { StrictMode } from "react";
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
import { ComputerHomePage } from "./ComputerHomePage";

vi.mock("@/shared/api/desktop-client", () => ({ getDesktopBridge: () => undefined }));
const list = vi.fn();
const readManagedWorker = vi.fn();
const createManagedWorker = vi.fn();
const startManagedWorker = vi.fn();
const deleteManagedWorker = vi.fn();
const worker = {
  instanceId: "talo-instance", nodeId: "talo-node", generation: 1, phase: "waitingForRegistration",
  operation: "create", cpuCores: 4, memoryBytes: 8 * 1024 ** 3, diskQuota: "unsupported", error: null,
} satisfies AppProtocol.ManagedWorkerInfo;
const query = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });

function Location() { return <output aria-label="Location">{useLocation().pathname}</output>; }
function show(path = "/files") {
  return render(<StrictMode><QueryClientProvider client={query}><MemoryRouter initialEntries={[path]}><ComputerHomePage /><Location /></MemoryRouter></QueryClientProvider></StrictMode>);
}

beforeEach(() => {
  void i18n.changeLanguage("en");
  list.mockReset().mockResolvedValue({ data: topologyNodes, total: 2, nextCursor: null });
  readManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: null });
  createManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: worker });
  startManagedWorker.mockReset();
  deleteManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: null });
  useAppStore.setState({
    selectedComputers: {}, workspaceWindows: [], workspaceSnapshots: {}, workspaceOwner: null, workspaceOpen: false,
    connection: { ...useAppStore.getInitialState().connection, authStatus: "authenticated", userId: "alice" },
  });
  setTalosClientFactory(() => ({
    computers: {
      list, readManagedWorker, createManagedWorker, startManagedWorker, deleteManagedWorker,
      read: async ({ nodeId }: { nodeId: string }) => nodeId === worker.nodeId
        ? { ...topologyNodes[0], nodeId, displayName: "Talo", managedWorkerPhase: "ready" }
        : topologyNodes.find(node => node.nodeId === nodeId),
    },
    sync: { status: async () => ({ localComputerId: null }) },
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
  await act(async () => reject(new TalosApiError({ code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Directory unavailable.", localization: null })));
  expect(await screen.findByRole("alert")).toHaveTextContent("Directory unavailable.");
  expect(screen.queryByRole("heading", { name: "Connect a personal computer" })).not.toBeInTheDocument();
  list.mockResolvedValue({ data: [], total: 0, nextCursor: null });
  fireEvent.click(screen.getByRole("button", { name: "Retry now" }));
  expect(await screen.findByRole("heading", { name: "Connect a personal computer" })).toBeVisible();
});

it("searches beyond the initial computer list without treating no matches as an unregistered account", async () => {
  const more = { ...topologyNodes[0], nodeId: "remote", displayName: "Remote office" };
  list.mockImplementation(async ({ query: search }) => search
    ? { data: search === "office" ? [more] : [], total: search === "office" ? 1 : 0, nextCursor: null }
    : { data: topologyNodes, total: 21, nextCursor: "next" });
  show();
  const search = await screen.findByRole("searchbox", { name: "Search personal computers" });
  expect(screen.queryByRole("button", { name: "Show more" })).not.toBeInTheDocument();
  fireEvent.change(search, { target: { value: "office" } });
  await waitFor(() => expect(list).toHaveBeenCalledWith(expect.objectContaining({ query: "office" })));
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

it("connects the ready Talo through the existing Computer workspace and shows its quota capability", async () => {
  readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "ready", operation: null } });
  show("/talo");
  const connect = await screen.findByRole("button", { name: "Connect desktop" });
  expect(connect.parentElement).toHaveTextContent("This runtime does not enforce a disk quota.");
  fireEvent.click(connect);
  await waitFor(() => expect(useAppStore.getState().workspaceWindows).toEqual([{ kind: "computer", nodeId: worker.nodeId }]));
  expect(useAppStore.getState().selectedComputers[connectionScope(useAppStore.getState().connection)]).toBe(worker.nodeId);
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
  await waitFor(() => expect(useAppStore.getState().workspaceWindows).toEqual([{ kind: "computer", nodeId: worker.nodeId }]));
  expect(startManagedWorker).toHaveBeenCalledExactlyOnceWith({ instanceId: worker.instanceId, expectedGeneration: worker.generation });
});

it("requires explicit confirmation for Talo deletion", async () => {
  readManagedWorker.mockResolvedValue({ enabled: true, instance: { ...worker, phase: "stopped", operation: null } });
  show("/talo");
  const button = await screen.findByRole("button", { name: "Delete Talo" });
  fireEvent.click(button);
  const dialog = await screen.findByRole("alertdialog", { name: "Delete Talo?" });
  expect(dialog).toHaveTextContent(/will be permanently deleted/);
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(deleteManagedWorker).not.toHaveBeenCalled();
  fireEvent.click(button);
  fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Delete Talo" }));
  await waitFor(() => expect(deleteManagedWorker).toHaveBeenCalledExactlyOnceWith({
    instanceId: worker.instanceId, expectedGeneration: worker.generation, confirmDataLoss: true,
  }));
});
