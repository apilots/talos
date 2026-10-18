// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ComputersPage } from "./ComputersPage";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { useAppStore } from "@/shared/store/app-store";
import { computerResources } from "@/shared/i18n/computerResources";
import {
  topologyMetrics,
  topologyNodes,
} from "@/shared/test/computer-topology.fixture";

const mocks = vi.hoisted(() => ({
  select: vi.fn(),
  list: vi.fn(),
  next: vi.fn(),
  metrics: vi.fn(),
  status: vi.fn(),
}));
vi.mock("@/shared/api/desktop-client", () => ({
  getDesktopBridge: () => undefined,
}));
vi.mock("@/features/computers/hooks", async (original) => ({
  ...(await original<typeof import("@/features/computers/hooks")>()),
  useComputers: () => ({
    scope: "fixture",
    selectedId: "manager-a",
    localId: "manager-a",
    select: mocks.select,
  }),
  useComputerList: mocks.list,
  useProductScope: () => "fixture",
  useComputer: (id: string) => ({
    data: topologyNodes.find((node) => node.nodeId === id),
    error: null,
  }),
  useAccountSyncMetrics: mocks.metrics,
  useSyncStatus: mocks.status,
}));

beforeEach(() => {
  vi.clearAllMocks();
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

async function show(locale = "en") {
  const i18n = createInstance();
  await i18n.init({
    lng: locale,
    resources: {
      [locale]: {
        computers: computerResources[locale as keyof typeof computerResources],
        common: { loading: "Loading", save: "Save", showMore: "More" },
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

it("keeps inspection across topology/list and changes execution selection only when opening", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: "Inspect Build Worker" }));
  expect(mocks.select).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "List" }));
  expect(screen.getByRole("button", { name: "Build Worker" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(screen.getByRole("heading", { name: "Build Worker" })).toBeVisible();
  fireEvent.change(screen.getByRole("textbox", { name: "Search computers" }), {
    target: { value: "worker" },
  });
  expect(mocks.list).toHaveBeenLastCalledWith("worker");
  fireEvent.click(screen.getByRole("button", { name: /More/ }));
  expect(mocks.next).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "Open computer" }));
  expect(mocks.select).toHaveBeenCalledExactlyOnceWith("worker-b");
  await waitFor(() =>
    expect(screen.getByText("Workspace opened")).toBeVisible(),
  );
});

it("renders the Chinese management surface and hides cached sync mappings when unavailable", async () => {
  mocks.metrics.mockReturnValue({
    data: undefined,
    enabled: false,
    isPending: false,
    error: null,
  });
  const { container } = await show("zh-CN");
  expect(screen.getByRole("button", { name: "拓扑" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(screen.getByText("登录后查看个人账号的同步拓扑。")).toBeVisible();
  expect(container.querySelectorAll('[data-flow="sync"]')).toHaveLength(0);
  expect(container.querySelectorAll('[data-flow="execution"]')).toHaveLength(2);
});

it.each(["zh-CN", "en"])("keeps the last endpoint and one localized disconnection notice in %s without stale sync paths", async (locale) => {
  const failure = new TalosApiError({ code: AppProtocol.ErrorCode.ServiceUnavailable, message: "The synchronization metrics service is unavailable.", localization: null });
  mocks.status.mockReturnValue({ data: { connection: "disconnected", configuredEnabled: true, pendingChanges: 0, lastError: failure.apiError }, isPending: false });
  mocks.metrics.mockReturnValue({
    data: undefined,
    endpoint: topologyMetrics.endpoint,
    enabled: true,
    isPending: false,
    error: new TalosApiError({
      code: AppProtocol.ErrorCode.ServiceUnavailable,
      message: "The synchronization metrics service is unavailable.",
      localization: null,
    }),
  });
  const { container } = await show(locale);
  const labels = computerResources[locale as keyof typeof computerResources];
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(screen.getByRole("alert")).toHaveTextContent(labels.syncUnavailable);
  expect(screen.getByRole("status")).toHaveTextContent(labels.syncDisconnected);
  expect(screen.queryByText("The synchronization metrics service is unavailable.")).toBeNull();
  expect(screen.getByRole("button", { name: locale === "en" ? "Inspect Talos Sync" : "查看 Talos Sync 的详情" })).toHaveTextContent(labels.lastKnownEndpointShort);
  fireEvent.click(screen.getByRole("button", { name: locale === "en" ? "Inspect Talos Sync" : "查看 Talos Sync 的详情" }));
  expect(screen.getAllByText(topologyMetrics.endpoint)).toHaveLength(2);
  expect(screen.getByText(labels.lastKnownEndpoint)).toBeVisible();
  expect(screen.getAllByRole("alert")).toHaveLength(1);

  expect(container.querySelectorAll('[data-flow="sync"]')).toHaveLength(0);
  expect(container.querySelectorAll('[data-flow="execution"]')).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: locale === "en" ? "Inspect Studio" : "查看 Studio 的详情" }));
  expect(screen.getByRole("button", { name: locale === "en" ? "Open computer" : "进入电脑" })).toBeEnabled();
});
