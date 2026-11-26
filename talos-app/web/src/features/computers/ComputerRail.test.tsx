// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ComputerRail } from "./ComputerRail";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";

const mocks = vi.hoisted(() => ({ computers: vi.fn(), list: vi.fn() }));
vi.mock("./hooks", () => ({
  useComputers: mocks.computers,
  useComputerList: mocks.list,
}));
vi.mock("@/features/auth", () => ({ useLogout: () => vi.fn() }));
vi.mock("@/widgets/sidebar/SidebarAccountMenu", () => ({ SidebarAccountMenu: () => null }));

const quickNodes = [...topologyNodes, { ...topologyNodes[0], nodeId: "third", displayName: "Third Computer" }];

beforeEach(() => {
  mocks.list.mockReturnValue({ data: { pages: [{ data: [], total: 0 }] } });
  mocks.computers.mockReturnValue({ quickNodes, total: 9, select: vi.fn() });
});
afterEach(cleanup);

async function show(locale = "en") {
  const i18n = createInstance();
  await i18n.init({
    lng: locale,
    resources: { [locale]: { computers: computerResources[locale as keyof typeof computerResources], common: { retryNow: locale === "en" ? "Retry" : "立即重试" } } },
    interpolation: { escapeValue: false },
  });
  return render(<I18nextProvider i18n={i18n}><MemoryRouter><ComputerRail /></MemoryRouter></I18nextProvider>);
}

it.each(["en", "zh-CN"])("shows a compact additional-count badge with an accessible name (%s)", async (locale) => {
  await show(locale);
  const more = locale === "en" ? "6 more" : "还有 6 台";
  const all = locale === "en" ? "All computers" : "全部 Computer";
  const trigger = screen.getByRole("button", { name: `${all} · ${more}` });
  expect(trigger).toHaveTextContent("+6");
  expect(trigger).not.toHaveAttribute("title");
  expect(screen.getByText("+6")).toBeVisible();
  fireEvent.click(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByRole("dialog")).toBeVisible();
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

it.each([0, 1, 3])("does not claim additional computers when all %i fit in the rail", async (total) => {
  mocks.computers.mockReturnValue({ quickNodes: quickNodes.slice(0, total), total, select: vi.fn() });
  await show("zh-CN");
  expect(screen.getByRole("button", { name: "全部 Computer" })).toBeVisible();
  expect(screen.queryByText(/\+\d/)).not.toBeInTheDocument();
});

it.for(["en", "zh-CN"])("shows a red network warning and retries the directory from the rail (%s)", async (locale, { expect }) => {
  const retry = vi.fn();
  mocks.computers.mockReturnValue({ quickNodes: quickNodes.slice(0, 1), total: 1, localId: quickNodes[0].nodeId, select: vi.fn(), list: { error: new TypeError("Failed to fetch"), refetch: retry } });
  const { container } = await show(locale);
  const message = computerResources[locale as keyof typeof computerResources].networkError;
  expect(screen.getByRole("alert")).toHaveClass("text-danger");
  expect(screen.getByRole("alert")).toHaveTextContent(message);
  fireEvent.click(screen.getByRole("button", { name: `${message} ${locale === "en" ? "Retry" : "立即重试"}` }));
  expect(retry).toHaveBeenCalledOnce();
  expect(screen.queryByText("Third Computer")).not.toBeInTheDocument();
  expect(container.querySelector('[role="alert"]')?.outerHTML).toBe(`<div role="alert" class="text-danger"><span class="sr-only">${message}</span><button type="button" title="${message}" aria-label="${message} ${locale === "en" ? "Retry" : "立即重试"}" class="flex size-10 items-center justify-center rounded-xl hover:bg-hover focus-visible:outline focus-visible:outline-2"><svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide lucide-circle-alert" aria-hidden="true"><circle cx="12" cy="12" r="10"></circle><line x1="12" x2="12" y1="8" y2="12"></line><line x1="12" x2="12.01" y1="16" y2="16"></line></svg></button></div>`);
});

it.each([true, false])("uses Desktop-only runtime authority consistently in rail and overflow (Desktop=%s)", async (desktop) => {
  const node = { ...quickNodes[0], available: true };
  mocks.computers.mockReturnValue({
    quickNodes: [node], total: 1, select: vi.fn(), localId: node.nodeId,
    localNode: desktop ? node : undefined, runtime: { kind: "restarting", message: null },
  });
  mocks.list.mockReturnValue({ data: { pages: [{ data: [node], total: 1 }] } });
  await show();
  const name = desktop ? "Studio · This computer · Restarting…" : "Studio · Connected";
  expect(screen.getByRole("button", { name })).toHaveAttribute("title", name);
  fireEvent.click(screen.getByRole("button", { name: "All computers" }));
  expect(within(screen.getByRole("dialog")).getByRole("button", { name })).toHaveTextContent(desktop ? "Restarting…" : "Connected");
});

it("leaves sidebar presentation to application navigation", async () => {
 const { container } = await show();
 expect(container.querySelector('[aria-controls="talos-sidebar"]')).toBeNull();
});
