// @vitest-environment jsdom
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { mocks, show } from "./ComputersPage.test-support";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { useAppStore } from "@/shared/store/app-store";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";

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

it.each(["Topology", "List"])("passes a remote listed row to the pending inspector in %s view", async (view) => {
  mocks.read.mockImplementation((_id: string, listed?: AppProtocol.NodeInfo) => ({ data: listed, isPlaceholderData: true, error: null }));
  await show();
  if (view === "List") fireEvent.click(screen.getByRole("button", { name: "List" }));
  fireEvent.click(screen.getByRole("button", { name: view === "List" ? "Build Worker" : "Inspect Build Worker" }));
  expect(mocks.read).toHaveBeenLastCalledWith("worker-b", topologyNodes[1]);
  expect(screen.getByRole("heading", { name: "Build Worker" })).toBeVisible();
  expect(screen.queryByText("Loading")).not.toBeInTheDocument();
  expect(screen.queryByRole("textbox", { name: "Rename" })).not.toBeInTheDocument();
  expect(mocks.select).not.toHaveBeenCalled();
});

it("retains loaded Computers and inspection when loading another page fails", async () => {
  mocks.list.mockReturnValue({ data: { pages: [{ data: topologyNodes, total: 23 }] }, error: new TypeError("Network unavailable"),
    isFetchNextPageError: true, hasNextPage: true, fetchNextPage: mocks.next });
  await show();
  fireEvent.click(screen.getByRole("button", { name: "List" }));
  fireEvent.click(screen.getByRole("button", { name: "Build Worker" }));
  expect(screen.getByRole("heading", { name: "Build Worker" })).toBeVisible();
  expect(screen.getByRole("alert")).toHaveTextContent(computerResources.en.networkError);
  fireEvent.click(screen.getByRole("button", { name: /More/ }));
  expect(mocks.next).toHaveBeenCalledOnce();
});

it("shows registration actions only to the Computer owner", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: "Inspect Build Worker" }));
  expect(screen.getByRole("button", { name: "Remove registration" })).toBeVisible();
  act(() => {
    useAppStore.setState({
      connection: { ...useAppStore.getState().connection, userId: "bob" },
    });
  });
  await waitFor(() => expect(screen.queryByRole("button", { name: "Remove registration" })).not.toBeInTheDocument());
  expect(screen.queryByRole("button", { name: "Disable" })).not.toBeInTheDocument();
});


it.each(["zh-CN", "en"])("distinguishes matched and account totals through zero results and clearing search (%s)", async (locale) => {
  mocks.list.mockImplementation((query: string) => ({
    data: { pages: [{ data: query === "missing" ? [] : query ? topologyNodes.slice(0, 2) : topologyNodes,
      total: query === "missing" ? 0 : query ? 2 : 9 }] },
    hasNextPage: false,
    fetchNextPage: mocks.next,
  }));
  await show(locale);
  const labels = computerResources[locale as keyof typeof computerResources];
  const overview = screen.getByRole("region", { name: labels.title });
  const search = screen.getByRole("textbox", { name: labels.search });
  const summary = within(search.closest(".cmp-search-area")!).getByRole("status");
  expect(summary).toBeEmptyDOMElement();
  expect(summary).toHaveClass("sr-only");
  expect(within(overview).getByText("9")).toBeVisible();
  fireEvent.change(search, { target: { value: "worker" } });
  expect(screen.getByText(locale === "en" ? "2 matching, 9 total" : "找到 2 台，共 9 台")).toBe(summary);
  expect(summary).toBeVisible();
  expect(summary).toHaveAttribute("role", "status");
  expect(summary).toHaveTextContent(locale === "en" ? "2 matching, 9 total" : "找到 2 台，共 9 台");
  expect(within(summary.parentElement!).getByRole("textbox", { name: labels.search })).toBe(search);
  expect(within(overview).getByText("9")).toBeVisible();
  fireEvent.change(search, { target: { value: "missing" } });
  expect(screen.getByText(locale === "en" ? "0 matching, 9 total" : "找到 0 台，共 9 台")).toBeVisible();
  expect(screen.getByText(labels.graphEmpty)).toBeVisible();
  expect(within(overview).getByText("9")).toBeVisible();
  fireEvent.change(search, { target: { value: "  " } });
  expect(screen.queryByText(/matching,|找到 \d/)).not.toBeInTheDocument();
  expect(summary).toBeInTheDocument();
  expect(summary).toBeEmptyDOMElement();
  expect(summary).toHaveClass("sr-only");
  expect(within(overview).getByText("9")).toBeVisible();
  expect(mocks.list).toHaveBeenLastCalledWith("");
  fireEvent.change(search, { target: { value: "worker" } });
  expect(screen.getByText(locale === "en" ? "2 matching, 9 total" : "找到 2 台，共 9 台")).toBe(summary);
});

it("keeps filtered pagination counts distinct from the account total", async () => {
  mocks.list.mockImplementation((query: string) => ({
    data: { pages: [{ data: query ? topologyNodes.slice(0, 2) : topologyNodes, total: query ? 5 : 9 }] },
    hasNextPage: true,
    fetchNextPage: mocks.next,
  }));
  await show("zh-CN");
  fireEvent.change(screen.getByRole("textbox", { name: "搜索 Computer" }), { target: { value: "worker" } });
  expect(screen.getByText("找到 5 台，共 9 台")).toBeVisible();
  expect(screen.getAllByText(/已加载 2 \/ 5 台 Computer/)).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "More · 已加载 2 / 5 台 Computer" }));
  expect(mocks.next).toHaveBeenCalledOnce();
});

it.for(["en", "zh-CN"])("keeps only the local Computer after an initial directory failure and filters it (%s)", async (locale, { expect }) => {
  const retry = vi.fn();
  const error = new TalosApiError({ code: AppProtocol.ErrorCode.Timeout, message: "Request timed out", localization: null });
  mocks.list.mockReturnValue({ data: undefined, error, refetch: retry, hasNextPage: false });
  mocks.computers.mockReturnValue({ scope: "fixture", localId: "manager-a", localNode: topologyNodes[0], nodes: topologyNodes.slice(0, 1), total: 1, list: { error }, select: mocks.select });
  const { container } = await show(locale);
  const labels = computerResources[locale as keyof typeof computerResources];
  expect(screen.getByRole("alert")).toHaveTextContent(labels.networkError);
  expect(screen.getByRole("alert")).toHaveClass("text-danger");
  expect(container.querySelectorAll("[data-flow=execution]")).toHaveLength(0);
  expect(screen.queryByRole("button", { name: locale === "en" ? "Inspect Build Worker" : "查看 Build Worker 的详情" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: locale === "en" ? "Inspect Studio" : "查看 Studio 的详情" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /More/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: locale === "en" ? "Retry" : "立即重试" }));
  expect(retry).toHaveBeenCalledOnce();
  expect(screen.getByRole("alert").outerHTML).toBe(`<div role="alert" class="cmp-notice text-sm text-danger"><span>${labels.networkError}</span><button type="button" class="ml-2 underline disabled:opacity-50">${locale === "en" ? "Retry" : "立即重试"}</button></div>`);
  fireEvent.click(screen.getByRole("button", { name: locale === "en" ? "List" : "列表" }));
  expect(screen.getByRole("button", { name: "Studio" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Build Worker" })).not.toBeInTheDocument();
  const search = screen.getByRole("textbox", { name: labels.search });
  fireEvent.change(search, { target: { value: "STUDIO" } });
  expect(screen.getByRole("button", { name: "Studio" })).toBeVisible();
  fireEvent.change(search, { target: { value: "worker" } });
  expect(screen.queryByRole("button", { name: "Studio" })).not.toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent(labels.networkError);
});

it("drops an already inspected remote detail when the directory fails, even if its detail read succeeded", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: "Inspect Build Worker" }));
  expect(screen.getByRole("heading", { name: "Build Worker" })).toBeVisible();
  const error = new TypeError("Failed to fetch");
  mocks.list.mockReturnValue({ data: undefined, error, refetch: vi.fn() });
  mocks.computers.mockReturnValue({ scope: "fixture", localId: "manager-a", localNode: topologyNodes[0], nodes: topologyNodes.slice(0, 1), total: 1, list: { error }, select: mocks.select });
  fireEvent.click(screen.getByRole("button", { name: "List" }));
  expect(screen.queryByRole("heading", { name: "Build Worker" })).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Studio" })).toBeVisible();
  expect(mocks.select).not.toHaveBeenCalled();
});

it("does not present cached filtered totals as a successful search after a directory failure", async () => {
  mocks.list.mockImplementation((query: string) => ({
    data: { pages: [{ data: topologyNodes.slice(0, 2), total: 2 }] },
    error: query ? new Error("Directory unavailable") : undefined,
    hasNextPage: false,
  }));
  await show("zh-CN");
  fireEvent.change(screen.getByRole("textbox", { name: "搜索 Computer" }), { target: { value: "broken" } });
  expect(screen.queryByText(/找到 \d/)).not.toBeInTheDocument();
  expect(screen.queryByText(/已加载/)).not.toBeInTheDocument();
  expect(screen.getByRole("alert")).toBeVisible();
});


it.each(["loading", "error"])("keeps an unavailable account total unknown during %s", async (state) => {
  mocks.computers.mockReturnValue({
    scope: "fixture", total: 0, list: state === "loading" ? {} : { data: { pages: [{ total: 0 }] }, error: new Error("Directory unavailable") },
    select: mocks.select,
  });
  await show("zh-CN");
  const overview = screen.getByRole("region", { name: "Computer" });
  expect(overview.querySelector("strong")).toHaveTextContent("—");
  fireEvent.change(screen.getByRole("textbox", { name: "搜索 Computer" }), { target: { value: "worker" } });
  expect(screen.queryByText(/找到 \d/)).not.toBeInTheDocument();
});
