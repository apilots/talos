// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { ChatSessionPage } from "./ChatSessionPage";
import { getChatSessionPageMocks, resetChatSessionPageMocks } from "./ChatSessionPage.test-support";

const api = vi.hoisted(() => ({ identity: vi.fn(), read: vi.fn() }));
vi.mock("@/shared/api/desktop-client", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/shared/api/desktop-client")>(),
  getDesktopBridge: () => ({}),
  isDesktopRuntime: () => true,
}));
vi.mock("@talos/sdk-app-server/desktop", async (importOriginal) => ({
  ...await importOriginal<typeof import("@talos/sdk-app-server/desktop")>(),
  createElectronAppServerClient: () => ({ desktop: { workerIdentity: api.identity } }),
}));
vi.mock("@/features/chat", async (importOriginal) => {
  const support = await import("./ChatSessionPage.test-support");
  return support.createChatFeatureMock(importOriginal);
});
vi.mock("@/features/computers", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/features/computers")>(),
  useManagedWorkerStatus: () => ({ status: {} }),
}));
vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({
    computers: { read: api.read },
    config: { get: vi.fn().mockResolvedValue({ type: "sessionMessageSendPolicy", policy: {} }) },
  }),
}));

beforeEach(() => {
  vi.clearAllMocks();
  resetChatSessionPageMocks();
  Element.prototype.scrollIntoView = vi.fn();
  api.identity.mockResolvedValue({ nodeId: "desktop-worker", controlPlaneEndpoint: null });
  api.read.mockRejectedValue(new TypeError("Directory unavailable"));
  useAppStore.setState({
    selectedComputers: {},
    connection: {
      ...useAppStore.getInitialState().connection,
      authStatus: "authenticated",
      userId: "alice",
      workerRuntimeState: { kind: "ready", message: null },
    },
  });
});
afterEach(cleanup);

it.each([
  ["desktop-worker", "ready", false, "desktop-worker"],
  ["embedded", "ready", false, null],
  ["remote-worker", "ready", true, "desktop-worker"],
  ["desktop-worker", "restarting", true, "desktop-worker"],
] as const)("offline directory: %s with local runtime %s has composer disabled=%s", async (nodeId, kind, disabled, identityNodeId) => {
  api.identity.mockResolvedValue({ nodeId: identityNodeId, controlPlaneEndpoint: null });
  const mocks = getChatSessionPageMocks();
  mocks.session = { ...mocks.session as object, nodeId, sessionKind: "standard" };
  useAppStore.setState({ connection: {
    ...useAppStore.getState().connection, workerRuntimeState: { kind, message: null },
  } });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}>
    <MemoryRouter initialEntries={["/sessions/sess-1"]}>
      <Routes><Route path="/sessions/:sessionId" element={<ChatSessionPage />} /></Routes>
    </MemoryRouter>
  </QueryClientProvider>);

  await waitFor(() => expect(client.getQueryState([
    "talos", connectionScope(useAppStore.getState().connection), "computer", nodeId,
  ])?.status).toBe("error"));
  expect(api.identity).toHaveBeenCalled();
  await waitFor(() => expect(screen.getByTestId("chat-composer").getAttribute("data-disabled")).toBe(String(disabled)));
  if (!disabled) {
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(mocks.sendTurn).toHaveBeenCalledOnce());
  }
});
