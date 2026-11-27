// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  useComputers,
  useAccountSyncMetrics,
  useComputer,
} from "./hooks";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import {
  setTalosClientFactory,
  resetTalosClientFactory,
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
const sync = { status: vi.fn(), metrics: vi.fn() };
const nodes = ["server-worker", "desktop-worker"].map((nodeId) => ({ nodeId }));
function show() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return renderHook(useComputers, {
    wrapper: ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}
beforeEach(() => {
  vi.resetAllMocks();
  desktop.active = false;
  desktop.identity.mockResolvedValue({ nodeId: "desktop-worker", controlPlaneEndpoint: null });
  useAppStore.setState({
    selectedComputers: {},
    connection: {
      ...useAppStore.getInitialState().connection,
      authStatus: "authenticated",
      userId: "alice",
    },
  });
  computers.list.mockResolvedValue({ data: nodes, total: 2, nextCursor: null });
  sync.status.mockResolvedValue({ localComputerId: null });
  computers.read.mockImplementation(({ nodeId }: { nodeId: string }) =>
    Promise.resolve(nodes.find((node) => node.nodeId === nodeId) ?? { nodeId }),
  );
  setTalosClientFactory(
    () => ({ computers, sync }) as unknown as TalosWebClient,
  );
});
afterEach(() => {
  cleanup();
  resetTalosClientFactory();
});
it("selects the connected Server's own Worker as local in Web and keeps another Computer remote", async () => {
  sync.status.mockResolvedValue({ localComputerId: "server-worker" });
  const { result } = show();
  await waitFor(() => expect(result.current.selectedId).toBe("server-worker"));
  expect(result.current.localId).toBe("server-worker");
  result.current.select("desktop-worker");
  await waitFor(() => expect(result.current.selectedId).toBe("desktop-worker"));
  expect(result.current.localId).toBe("server-worker");
});
it("does not inject a shared Server's private Worker into another account's Computer list", async () => {
  sync.status.mockResolvedValue({ localComputerId: null });
  const { result } = show();
  await waitFor(() => expect(result.current.nodes).toHaveLength(2));
  expect(result.current.localId).toBeUndefined();
  expect(result.current.selectedId).toBeUndefined();
  expect(computers.read).not.toHaveBeenCalled();
});

it("uses Desktop process identity rather than the Server Worker as this computer", async () => {
  desktop.active = true;
  sync.status.mockResolvedValue({ localComputerId: "server-worker" });
  const { result } = show();
  await waitFor(() => expect(result.current.localId).toBe("desktop-worker"));
  await waitFor(() => expect(result.current.selectedId).toBe("desktop-worker"));
  expect(desktop.identity).toHaveBeenCalled();
  expect(result.current.nodes.map((node) => node.nodeId)).toEqual(["desktop-worker", "server-worker"]);
});

it("shows only the unregistered local Desktop on an initial cloud failure", async () => {
  desktop.active = true;
  desktop.identity.mockResolvedValue({ nodeId: null, controlPlaneEndpoint: null });
  computers.list.mockRejectedValue(new TypeError("Offline"));
  computers.read.mockRejectedValue(new TypeError("Offline"));
  const { result } = show();
  await waitFor(() => expect(result.current.list.error).toBeTruthy());
  expect(result.current.nodes.map((node) => node.nodeId)).toEqual(["embedded"]);
  expect(result.current.total).toBe(1);
});

it("reads details from the Server without filling them from a cached directory row", async () => {
  const scope = connectionScope(useAppStore.getState().connection);
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  query.setQueryData(["talos", scope, "computers", ""], {
    pages: [{ data: nodes, total: 2, nextCursor: null }], pageParams: [null],
  });
  computers.read.mockImplementation(() => new Promise(() => {}));
  const { result } = renderHook(() => useComputer("server-worker"), {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(computers.read).toHaveBeenCalledWith({ nodeId: "server-worker" }, expect.any(AbortSignal)));
  expect(result.current.data).toBeUndefined();
  expect(result.current.isPlaceholderData).toBe(false);
  query.clear();
});

it("preserves selection but withdraws remote detail after a cloud timeout", async () => {
  sync.status.mockResolvedValue({ localComputerId: null });
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.getState().selectComputer(scope, "server-worker");
  computers.read.mockRejectedValue(new TalosApiError({ code: "timeout", message: "Timeout", localization: null }));
  const { result } = show();
  await waitFor(() => expect(result.current.error).toBeTruthy());
  expect(result.current.selected).toBeUndefined();
  expect(result.current.selectedId).toBe("server-worker");
});

it("hides stale Computer detail after a failed refetch and restores it after recovery", async () => {
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(() => useComputer("server-worker"), {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(result.current.data?.nodeId).toBe("server-worker"));
  computers.read.mockRejectedValue(new TypeError("Offline"));
  await act(async () => { await result.current.refetch(); });
  await waitFor(() => expect(result.current.error).toBeTruthy());
  expect(result.current.data).toBeUndefined();
  computers.read.mockResolvedValue({ nodeId: "server-worker", displayName: "Updated" });
  await act(async () => { await result.current.refetch(); });
  await waitFor(() => expect(result.current.data?.displayName).toBe("Updated"));
});

it("clears a remote selection only on an authoritative Server directory miss", async () => {
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.getState().selectComputer(scope, "server-worker");
  computers.read.mockRejectedValue(new TalosApiError({ code: "resourceNotFound", message: "Not found", localization: null }));
  const { result } = show();
  await waitFor(() => expect(result.current.selectedId).toBe(""));
});

it("removes cached private metrics immediately when authentication ends", async () => {
  sync.metrics.mockResolvedValue({ endpoint: "https://private-sync.example", peers: [{ peerId: "private-peer" }] });
  const query = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const { result } = renderHook(useAccountSyncMetrics, {
    wrapper: ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={query}>{children}</QueryClientProvider>
    ),
  });
  await waitFor(() =>
    expect(result.current.data?.peers[0].peerId).toBe("private-peer"),
  );
  act(() =>
    { useAppStore.setState({
      connection: {
        ...useAppStore.getState().connection,
        authStatus: "logged_out",
      },
    }); },
  );
  expect(result.current.data).toBeUndefined();
  expect(result.current.enabled).toBe(false);
  expect(result.current.endpoint).toBeUndefined();
});

it("keeps a late previous-account metrics reply out of the current account view", async () => {
  let finishAlice!: (value: unknown) => void;
  let finishBob!: (value: unknown) => void;
  sync.metrics
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishAlice = resolve;
        }),
    )
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishBob = resolve;
        }),
    );
  const query = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const { result } = renderHook(useAccountSyncMetrics, {
    wrapper: ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={query}>{children}</QueryClientProvider>
    ),
  });
  await waitFor(() => expect(sync.metrics).toHaveBeenCalledTimes(1));
  act(() =>
    { useAppStore.setState({
      connection: { ...useAppStore.getState().connection, userId: "bob" },
    }); },
  );
  expect(result.current.data).toBeUndefined();
  await waitFor(() => expect(sync.metrics).toHaveBeenCalledTimes(2));
  await act(() => Promise.resolve(finishBob({ peers: [{ peerId: "bob-peer" }] })));
  await waitFor(() =>
    expect(result.current.data?.peers[0].peerId).toBe("bob-peer"),
  );
  await act(() =>
    Promise.resolve(finishAlice({ peers: [{ peerId: "alice-private-peer" }] })),
  );
  expect(result.current.data?.peers[0].peerId).toBe("bob-peer");
});

it("preserves a local UUID selection while Desktop identity is pending", async () => {
  desktop.active = true;
  let finish!: (value: { nodeId: string }) => void;
  desktop.identity.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.setState({ selectedComputers: { [scope]: "desktop-worker" } });
  computers.read.mockRejectedValue(new TalosApiError({ code: AppProtocol.ErrorCode.ResourceNotFound, message: "missing", localization: null }));
  const { result } = show();
  await waitFor(() => expect(result.current.error).toBeTruthy());
  expect(result.current.selectedId).toBe("desktop-worker");
  await act(() => Promise.resolve(finish({ nodeId: "desktop-worker" })));
  await waitFor(() => expect(result.current.localId).toBe("desktop-worker"));
  expect(result.current.selectedId).toBe("desktop-worker");
});

it("preserves live local registration fields outside the loaded directory page", async () => {
  desktop.active = true;
  computers.list.mockResolvedValue({ data: [{ nodeId: "server-worker" }], total: 25, nextCursor: "next" });
  computers.read.mockResolvedValue({ nodeId: "desktop-worker", displayName: "Registered local", ownerUserId: "alice", mode: "remote", status: "disabled", available: false });
  const { result } = show();
  await waitFor(() => expect(result.current.selected?.displayName).toBe("Registered local"));
  expect(result.current.selected).toMatchObject({ ownerUserId: "alice", mode: "remote", status: "disabled" });
  expect(result.current.quickNodes[0]).toMatchObject({ displayName: "Registered local", status: "disabled" });
});

it("retains only the scoped endpoint after a failed refetch and replaces it on recovery", async () => {
  sync.metrics.mockResolvedValue({ endpoint: "https://sync-a.example", peers: [{ peerId: "live-peer" }] });
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(useAccountSyncMetrics, { wrapper: ({ children }: PropsWithChildren) =>
    <QueryClientProvider client={query}>{children}</QueryClientProvider> });
  await waitFor(() => expect(result.current.endpoint).toBe("https://sync-a.example"));
  sync.metrics.mockRejectedValue(new Error("Connection refused"));
  await act(async () => { await query.refetchQueries(); });
  await waitFor(() => expect(result.current.error).toBeTruthy());
  expect(result.current.endpoint).toBe("https://sync-a.example");
  expect(result.current.data).toBeUndefined();
  sync.metrics.mockResolvedValue({ endpoint: "https://sync-b.example", peers: [] });
  await act(async () => { await query.refetchQueries(); });
  await waitFor(() => expect(result.current.endpoint).toBe("https://sync-b.example"));
  expect(result.current.data?.peers).toEqual([]);
  // A different host/account starts without the previous endpoint even on failure.
  sync.metrics.mockRejectedValue(new Error("New scope unavailable"));
  act(() => { useAppStore.setState({ connection: { ...useAppStore.getState().connection, controlPlaneEndpoint: "https://other-host.example" } }); });
  expect(result.current.endpoint).toBeUndefined();
  await waitFor(() => expect(result.current.error).toBeTruthy());
  expect(result.current.endpoint).toBeUndefined();
  act(() => { useAppStore.setState({ connection: { ...useAppStore.getState().connection, userId: "bob" } }); });
  expect(result.current.endpoint).toBeUndefined();
});
