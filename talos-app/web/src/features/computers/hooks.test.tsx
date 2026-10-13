// @vitest-environment jsdom
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useComputers, useCanManageComputer } from "./hooks";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { setTalosClientFactory, resetTalosClientFactory, type TalosWebClient } from "@/shared/api/client";

vi.mock("@/shared/api/desktop-client", () => ({ getDesktopBridge: () => undefined }));
const computers = { list: vi.fn(), read: vi.fn() };
const sync = { status: vi.fn() };
const nodes = ["server-worker", "desktop-worker"].map((nodeId) => ({ nodeId }));
function show() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return renderHook(useComputers, {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={client}>{children}</QueryClientProvider>,
  });
}
beforeEach(() => {
  vi.resetAllMocks();
  useAppStore.setState({ selectedComputers: {}, connection: { ...useAppStore.getInitialState().connection, authStatus: "authenticated", userId: "alice" } });
  computers.list.mockResolvedValue({ data: nodes, total: 2, nextCursor: null });
  computers.read.mockImplementation(({ nodeId }: { nodeId: string }) => Promise.resolve(nodes.find((node) => node.nodeId === nodeId)));
  setTalosClientFactory(() => ({ computers, sync } as unknown as TalosWebClient));
});
afterEach(() => { cleanup(); resetTalosClientFactory(); });
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
  const connection = { ...useAppStore.getState().connection, deploymentMode: "cloud" as const };
  useAppStore.setState({ connection });
  useAppStore.getState().selectComputer(connectionScope(connection), "owned-desktop");
  computers.read.mockResolvedValue({ nodeId: "owned-desktop", mode: "embedded", ownerUserId: "alice" });
  sync.status.mockResolvedValue({ localComputerId: "owned-desktop" });
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(useCanManageComputer, {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(result.current).toBe(true));
});
it("does not grant a deployment mode access to the center private Computer", async () => {
  const connection = { ...useAppStore.getState().connection, deploymentMode: "local" as const };
  useAppStore.setState({ connection });
  useAppStore.getState().selectComputer(connectionScope(connection), "embedded");
  computers.read.mockResolvedValue({ nodeId: "embedded", mode: "embedded", ownerUserId: null });
  sync.status.mockResolvedValue({ localComputerId: null });
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { result } = renderHook(useCanManageComputer, {
    wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={query}>{children}</QueryClientProvider>,
  });
  await waitFor(() => expect(result.current).toBe(false));
});
