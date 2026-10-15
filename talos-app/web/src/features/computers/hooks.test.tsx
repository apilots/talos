// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  useComputers,
  useCanManageComputer,
  useAccountSyncMetrics,
} from "./hooks";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import {
  setTalosClientFactory,
  resetTalosClientFactory,
  type TalosWebClient,
} from "@/shared/api/client";

vi.mock("@/shared/api/desktop-client", () => ({
  getDesktopBridge: () => undefined,
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
  useAppStore.setState({
    selectedComputers: {},
    connection: {
      ...useAppStore.getInitialState().connection,
      authStatus: "authenticated",
      userId: "alice",
    },
  });
  computers.list.mockResolvedValue({ data: nodes, total: 2, nextCursor: null });
  computers.read.mockImplementation(({ nodeId }: { nodeId: string }) =>
    Promise.resolve(nodes.find((node) => node.nodeId === nodeId)),
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

it("allows the owner of an embedded Computer to configure it in Cloud mode", async () => {
  const connection = {
    ...useAppStore.getState().connection,
    deploymentMode: "cloud" as const,
  };
  useAppStore.setState({ connection });
  useAppStore
    .getState()
    .selectComputer(connectionScope(connection), "owned-desktop");
  computers.read.mockResolvedValue({
    nodeId: "owned-desktop",
    mode: "embedded",
    ownerUserId: "alice",
  });
  sync.status.mockResolvedValue({ localComputerId: "owned-desktop" });
  const query = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const { result } = renderHook(useCanManageComputer, {
    wrapper: ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={query}>{children}</QueryClientProvider>
    ),
  });
  await waitFor(() => expect(result.current).toBe(true));
});
it("does not grant a deployment mode access to the center private Computer", async () => {
  const connection = {
    ...useAppStore.getState().connection,
    deploymentMode: "local" as const,
  };
  useAppStore.setState({ connection });
  useAppStore
    .getState()
    .selectComputer(connectionScope(connection), "embedded");
  computers.read.mockResolvedValue({
    nodeId: "embedded",
    mode: "embedded",
    ownerUserId: null,
  });
  sync.status.mockResolvedValue({ localComputerId: null });
  const query = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const { result } = renderHook(useCanManageComputer, {
    wrapper: ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={query}>{children}</QueryClientProvider>
    ),
  });
  await waitFor(() => expect(result.current).toBe(false));
});

it("removes cached private metrics immediately when authentication ends", async () => {
  sync.metrics.mockResolvedValue({ peers: [{ peerId: "private-peer" }] });
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
    useAppStore.setState({
      connection: {
        ...useAppStore.getState().connection,
        authStatus: "logged_out",
      },
    }),
  );
  expect(result.current.data).toBeUndefined();
  expect(result.current.enabled).toBe(false);
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
    useAppStore.setState({
      connection: { ...useAppStore.getState().connection, userId: "bob" },
    }),
  );
  expect(result.current.data).toBeUndefined();
  await waitFor(() => expect(sync.metrics).toHaveBeenCalledTimes(2));
  await act(async () => finishBob({ peers: [{ peerId: "bob-peer" }] }));
  await waitFor(() =>
    expect(result.current.data?.peers[0].peerId).toBe("bob-peer"),
  );
  await act(async () =>
    finishAlice({ peers: [{ peerId: "alice-private-peer" }] }),
  );
  expect(result.current.data?.peers[0].peerId).toBe("bob-peer");
});
