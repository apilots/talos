// @vitest-environment jsdom
import { fireEvent, screen, within } from "@testing-library/react";
import { expect, it } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { mocks, show } from "./ComputersPage.test-support";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyMetrics } from "@/shared/test/computer-topology.fixture";

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
  expect(within(screen.getByTestId("sync-control")).getByRole("status")).toHaveTextContent(labels.syncDisconnected);
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
