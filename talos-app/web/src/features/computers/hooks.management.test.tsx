// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TalosApiError } from "@talos/sdk-core";
import { useCanManageComputer, useComputers } from "./hooks";
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
const sync = { status: vi.fn() };
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
  computers.list.mockResolvedValue({ data: [], total: 0, nextCursor: null });
  sync.status.mockResolvedValue({ localComputerId: null });
  setTalosClientFactory(
    () => ({ computers, sync }) as unknown as TalosWebClient,
  );
});
afterEach(() => {
  cleanup();
  resetTalosClientFactory();
});

it("allows the owner of an embedded Computer to configure it regardless of host deployment", async () => {
  const connection = {
    ...useAppStore.getState().connection,
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
it.each([
  ["desktop-worker", "desktop-worker", true],
  [null, "embedded", true],
  ["desktop-worker", "remote-worker", false],
  ["desktop-worker", "embedded", false],
] as const)("offline directory: Desktop identity %s grants configuration of %s = %s", async (identityNodeId, selectedId, allowed) => {
  desktop.active = true;
  desktop.identity.mockResolvedValue({ nodeId: identityNodeId, controlPlaneEndpoint: null });
  computers.read.mockRejectedValue(new TypeError("Directory unavailable"));
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.getState().selectComputer(scope, selectedId);
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(useCanManageComputer, {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(query.getQueryState(["talos", scope, "local-computer"])?.status).toBe("success"));
  await waitFor(() => expect(query.getQueryState(["talos", scope, "computer", selectedId])?.status).toBe("error"));
  await waitFor(() => expect(result.current).toBe(allowed));
});

it.each([
  ["desktop-worker", true],
  ["remote-worker", false],
] as const)("keeps configuration of %s pending after an immediate directory miss until Desktop identity resolves", async (selectedId, allowed) => {
  desktop.active = true;
  let resolveIdentity!: (identity: { nodeId: string; controlPlaneEndpoint: null }) => void;
  desktop.identity.mockImplementation(() => new Promise((resolve) => { resolveIdentity = resolve; }));
  computers.read.mockRejectedValue(new TalosApiError({ code: "resourceNotFound", message: "Not found", localization: null }));
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.getState().selectComputer(scope, selectedId);
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(useCanManageComputer, {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(query.getQueryState(["talos", scope, "computer", selectedId])?.status).toBe("error"));
  expect(query.getQueryState(["talos", scope, "local-computer"])?.status).toBe("pending");
  expect(result.current).toBeUndefined();
  await act(() => { resolveIdentity({ nodeId: "desktop-worker", controlPlaneEndpoint: null });  return Promise.resolve(); });
  await waitFor(() => expect(result.current).toBe(allowed));
});

it("does not grant remote configuration from cached ownership when the directory fails", async () => {
  desktop.active = true;
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.getState().selectComputer(scope, "remote-worker");
  computers.read.mockResolvedValue({ nodeId: "remote-worker", ownerUserId: "alice" });
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(useCanManageComputer, {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(result.current).toBe(true));
  computers.read.mockRejectedValue(new TypeError("Directory unavailable"));
  await act(async () => { await query.refetchQueries({ queryKey: ["talos", scope, "computer", "remote-worker"] }); });
  await waitFor(() => expect(result.current).toBe(false));
});

it("keeps an offline identity display row without granting configuration when Desktop identity fails", async () => {
  desktop.active = true;
  desktop.identity.mockRejectedValue(new TypeError("Identity unavailable"));
  computers.list.mockRejectedValue(new TypeError("Directory unavailable"));
  computers.read.mockRejectedValue(new TypeError("Directory unavailable"));
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.getState().selectComputer(scope, "embedded");
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(() => ({ computers: useComputers(), canManage: useCanManageComputer() }), {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(result.current.computers.identity.isError).toBe(true));
  await waitFor(() => expect(result.current.computers.list.isError).toBe(true));
  expect(result.current.computers.nodes.map((node) => node.nodeId)).toEqual(["embedded"]);
  await waitFor(() => expect(result.current.canManage).toBe(false));
});

it("does not grant access to the center private Computer", async () => {
  const connection = {
    ...useAppStore.getState().connection,
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

it("retains local identity and configuration across a failed identity poll, then accepts recovery", async () => {
  desktop.active = true;
  const systemInfo = { hostname: "local-host" };
  desktop.identity.mockResolvedValue({ nodeId: "desktop-worker", systemInfo });
  computers.read.mockRejectedValue(new TalosApiError({ code: "resourceNotFound", message: "Not enrolled", localization: null }));
  const scope = connectionScope(useAppStore.getState().connection);
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(() => ({ computers: useComputers(), canManage: useCanManageComputer() }), {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(result.current.canManage).toBe(true));
  desktop.identity.mockRejectedValue(new TypeError("IPC interrupted"));
  await act(async () => { await query.refetchQueries({ queryKey: ["talos", scope, "local-computer"] }); });
  await waitFor(() => expect(result.current.computers.identity.isError).toBe(true));
  expect(result.current.computers.localId).toBe("desktop-worker");
  expect(result.current.computers.selectedId).toBe("desktop-worker");
  expect(result.current.computers.localNode?.systemInfo).toEqual(systemInfo);
  expect(result.current.canManage).toBe(true);
  desktop.identity.mockResolvedValue({ nodeId: "desktop-worker", systemInfo: { hostname: "recovered-host" } });
  await act(async () => { await query.refetchQueries({ queryKey: ["talos", scope, "local-computer"] }); });
  await waitFor(() => expect(result.current.computers.localNode?.systemInfo?.hostname).toBe("recovered-host"));
  query.clear();
});
