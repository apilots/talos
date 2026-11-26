// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { useComputer, useComputers } from "./hooks";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import {
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "@/shared/api/client";

const desktop = vi.hoisted(() => ({ active: false, identity: vi.fn() }));
vi.mock("@/shared/api/desktop-client", () => ({
  getDesktopBridge: () => desktop.active ? {} : undefined,
}));
vi.mock("@talos/sdk-app-server/desktop", async (importOriginal) => ({
  ...await importOriginal<typeof import("@talos/sdk-app-server/desktop")>(),
  createElectronAppServerClient: () => ({ desktop: { workerIdentity: desktop.identity } }),
}));

const computers = { list: vi.fn(), read: vi.fn() };
const sync = { status: vi.fn() };
const rows: AppProtocol.NodeInfo[] = Array.from({ length: 21 }, (_, index) => ({
  ...topologyNodes[index % topologyNodes.length], nodeId: `computer-${index}`,
}));
const listed = rows[1];
const page = (data = rows.slice(0, 20), total = 21, nextCursor: string | null = "after-20") =>
  ({ data, total, nextCursor });
const networkError = () => new TypeError("Network unavailable");
const directoryError = (code: AppProtocol.ErrorCode) =>
  new TalosApiError({ code, message: "Directory rejected", localization: null });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const clients: QueryClient[] = [];
function show<T>(hook: () => T) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  clients.push(client);
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { ...renderHook(hook, { wrapper }), wrapper };
}
function select(nodeId: string) {
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.setState({ selectedComputers: { [scope]: nodeId } });
}

beforeEach(() => {
  vi.resetAllMocks();
  desktop.active = false;
  desktop.identity.mockResolvedValue({ nodeId: rows[0].nodeId, controlPlaneEndpoint: null });
  useAppStore.setState({
    selectedComputers: {},
    connection: {
      ...useAppStore.getInitialState().connection, authStatus: "authenticated", userId: "alice",
    },
  });
  computers.list.mockResolvedValue(page());
  computers.read.mockImplementation(({ nodeId }: { nodeId: string }) =>
    Promise.resolve(rows.find((node) => node.nodeId === nodeId) ?? { ...topologyNodes[0], nodeId }),
  );
  sync.status.mockResolvedValue({ localComputerId: null });
  setTalosClientFactory(() => ({ computers, sync }) as unknown as TalosWebClient);
});
afterEach(() => {
  cleanup();
  clients.splice(0).forEach((client) => client.clear());
  resetTalosClientFactory();
});

describe.each(["Web", "Desktop"] as const)("%s directory failure", (environment) => {
  beforeEach(() => { desktop.active = environment === "Desktop"; });

  it.each(["initial", "refresh", "next-page", "denied"] as const)("withdraws remote rows after %s failure and restores them on retry", async (failure) => {
    select(listed.nodeId);
    if (failure === "initial") computers.list.mockRejectedValue(networkError());
    const { result } = show(useComputers);
    if (failure !== "initial") {
      await waitFor(() => expect(result.current.nodes).toHaveLength(20));
      expect(result.current.quickNodes.length).toBeLessThanOrEqual(3);
      computers.list.mockRejectedValue(failure === "denied"
        ? directoryError(AppProtocol.ErrorCode.PermissionDenied) : networkError());
      await act(async () => {
        if (failure === "next-page") await result.current.list.fetchNextPage();
        else await result.current.list.refetch();
      });
    }
    await waitFor(() => expect(result.current.list.isError).toBe(true));
    expect(result.current.list.data).toBeUndefined();
    expect(result.current.selected).toBeUndefined();
    const visibleIds = desktop.active ? [rows[0].nodeId] : [];
    expect(result.current.nodes.map((node) => node.nodeId)).toEqual(visibleIds);
    expect(result.current.quickNodes.map((node) => node.nodeId)).toEqual(visibleIds);
    computers.list.mockResolvedValue(page());
    await act(async () => { await result.current.list.refetch(); });
    await waitFor(() => expect(result.current.nodes).toHaveLength(20));
    expect(result.current.total).toBe(21);
    expect(result.current.list.error).toBeNull();
    expect(result.current.quickNodes.length).toBeLessThanOrEqual(3);
  });
});

it("shows a deferred detail's list placeholder immediately without caching it for authoritative callers", async () => {
  const detail = deferred<AppProtocol.NodeInfo>();
  computers.read.mockReturnValue(detail.promise);
  const provisional = show(() => useComputer(listed.nodeId, listed));
  expect(provisional.result.current.data).toEqual(listed);
  expect(provisional.result.current.isPlaceholderData).toBe(true);
  const authoritative = renderHook(() => useComputer(listed.nodeId), { wrapper: provisional.wrapper });
  expect(authoritative.result.current.data).toBeUndefined();
  expect(authoritative.result.current.isPending).toBe(true);
  await waitFor(() => expect(computers.read).toHaveBeenCalledTimes(1));
  const full: AppProtocol.NodeInfo = { ...listed, displayName: "Authoritative detail", updatedAt: 42 };
  await act(async () => { detail.resolve(full); });
  await waitFor(() => expect(provisional.result.current.data).toEqual(full));
  expect(provisional.result.current.isPlaceholderData).toBe(false);
  expect(authoritative.result.current.data).toEqual(full);
});

it("does not show a listed node for a different ID", async () => {
  const detail = deferred<AppProtocol.NodeInfo>();
  computers.read.mockReturnValue(detail.promise);
  const { result } = show(() => useComputer(rows[2].nodeId, listed));
  expect(result.current.data).toBeUndefined();
  expect(result.current.isPending).toBe(true);
  await act(async () => { detail.resolve(rows[2]); });
  await waitFor(() => expect(result.current.data).toEqual(rows[2]));
});

it("withdraws both provisional and authoritative detail after a transient failure", async () => {
  const detail = deferred<AppProtocol.NodeInfo>();
  computers.read.mockReturnValue(detail.promise);
  const provisional = show(() => useComputer(listed.nodeId, listed));
  const authoritative = renderHook(() => useComputer(listed.nodeId), { wrapper: provisional.wrapper });
  await act(async () => { detail.reject(networkError()); });
  await waitFor(() => expect(provisional.result.current.isError).toBe(true));
  expect(provisional.result.current.data).toBeUndefined();
  expect(authoritative.result.current.isError).toBe(true);
  expect(authoritative.result.current.data).toBeUndefined();
});

it("does not retain a previous authoritative detail on transient refetch failure without a listed argument", async () => {
  const { result } = show(() => useComputer(listed.nodeId));
  await waitFor(() => expect(result.current.data).toEqual(listed));
  computers.read.mockRejectedValue(networkError());
  await act(async () => { await result.current.refetch(); });
  await waitFor(() => expect(result.current.isError).toBe(true));
  expect(result.current.data).toBeUndefined();
});

it.each([
  AppProtocol.ErrorCode.ResourceNotFound,
  AppProtocol.ErrorCode.ResourceExpired,
  AppProtocol.ErrorCode.PermissionDenied,
])("hides provisional and previously authoritative detail on %s", async (code) => {
  const { result } = show(() => ({
    provisional: useComputer(listed.nodeId, listed),
    authoritative: useComputer(listed.nodeId),
  }));
  await waitFor(() => expect(result.current.authoritative.data).toEqual(listed));
  computers.read.mockRejectedValue(directoryError(code));
  await act(async () => { await result.current.provisional.refetch(); });
  await waitFor(() => expect(result.current.provisional.isError).toBe(true));
  expect(result.current.provisional.data).toBeUndefined();
  expect(result.current.authoritative.data).toBeUndefined();
});

it("never reuses an old account's directory as the new account's selected detail placeholder", async () => {
  select(listed.nodeId);
  const aliceDetail = deferred<AppProtocol.NodeInfo>();
  const bobDetail = deferred<AppProtocol.NodeInfo>();
  computers.read.mockReturnValueOnce(aliceDetail.promise).mockReturnValue(bobDetail.promise);
  const { result } = show(useComputers);
  await waitFor(() => expect(result.current.selected).toEqual(listed));
  const bobPage = deferred<ReturnType<typeof page>>();
  computers.list.mockReturnValue(bobPage.promise);
  act(() => {
    const connection = { ...useAppStore.getState().connection, userId: "bob" };
    useAppStore.setState({ connection, selectedComputers: { [connectionScope(connection)]: listed.nodeId } });
  });
  expect(result.current.list.data).toBeUndefined();
  expect(result.current.nodes).toEqual([]);
  expect(result.current.quickNodes).toEqual([]);
  expect(result.current.selected).toBeUndefined();
  await waitFor(() => expect(computers.list).toHaveBeenCalledTimes(2));
  const bobNode: AppProtocol.NodeInfo = { ...listed, ownerUserId: "bob", displayName: "Bob directory" };
  await act(async () => { bobPage.resolve(page([bobNode], 1, null)); });
  await waitFor(() => expect(result.current.selected).toEqual(bobNode));
  await act(async () => { aliceDetail.resolve({ ...listed, displayName: "Late Alice detail" }); });
  expect(result.current.nodes).toEqual([bobNode]);
  expect(result.current.selected).toEqual(bobNode);
  const full: AppProtocol.NodeInfo = { ...bobNode, displayName: "Bob detail" };
  await act(async () => { bobDetail.resolve(full); });
  await waitFor(() => expect(result.current.selected).toEqual(full));
});
