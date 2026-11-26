import "@testing-library/jest-dom/vitest";
import { cleanup, render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, vi } from "vitest";
import { ComputersPage } from "./ComputersPage";
import { useAppStore } from "@/shared/store/app-store";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyMetrics, topologyNodes } from "@/shared/test/computer-topology.fixture";

const mocks = vi.hoisted(() => ({
  select: vi.fn(),
  list: vi.fn(),
  next: vi.fn(),
  metrics: vi.fn(),
  status: vi.fn(),
  computers: vi.fn(),
  read: vi.fn(),
}));
export { mocks };
vi.mock("@/shared/api/desktop-client", () => ({
  getDesktopBridge: () => undefined,
}));
vi.mock("@/features/computers/hooks", async (original) => ({
  ...(await original<typeof import("@/features/computers/hooks")>()),
  useComputers: mocks.computers,
  useComputerList: mocks.list,
  useProductScope: () => "fixture",
  useComputer: mocks.read,
  useAccountSyncMetrics: mocks.metrics,
  useSyncStatus: mocks.status,
}));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.read.mockImplementation((id: string) => ({ data: topologyNodes.find((node) => node.nodeId === id), error: null }));
  mocks.computers.mockReturnValue({
    scope: "fixture", selectedId: "manager-a", localId: "manager-a",
    total: 9, nodes: topologyNodes, list: { data: { pages: [{ total: 9 }] } }, select: mocks.select,
  });
  useAppStore.setState({
    connection: {
      ...useAppStore.getInitialState().connection,
      authStatus: "authenticated",
      userId: "alice",
    },
  });
  mocks.list.mockReturnValue({
    data: { pages: [{ data: topologyNodes, total: 3 }] },
    hasNextPage: true,
    fetchNextPage: mocks.next,
  });
  mocks.status.mockReturnValue({ data: { connection: "disabled", configuredEnabled: false, pendingChanges: 0 }, isPending: false });
  mocks.metrics.mockReturnValue({
    data: topologyMetrics,
    endpoint: topologyMetrics.endpoint,
    enabled: true,
    isPending: false,
    error: null,
  });
});
afterEach(cleanup);

export async function show(locale = "en") {
  const i18n = createInstance();
  await i18n.init({
    lng: locale,
    resources: {
      [locale]: {
        computers: computerResources[locale as keyof typeof computerResources],
        common: { loading: "Loading", save: "Save", showMore: "More", retryNow: locale === "en" ? "Retry" : "立即重试" },
      },
    },
    interpolation: { escapeValue: false },
  });
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queries}>
      <I18nextProvider i18n={i18n}>
        <MemoryRouter initialEntries={["/computers"]}>
          <Routes>
            <Route path="/computers" element={<ComputersPage />} />
            <Route path="/" element={<p>Workspace opened</p>} />
          </Routes>
        </MemoryRouter>
      </I18nextProvider>
    </QueryClientProvider>,
  );
}
