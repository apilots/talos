// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ComputerRail } from "./ComputerRail";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";

const mocks = vi.hoisted(() => ({ computers: vi.fn() }));
vi.mock("./hooks", () => ({
  useComputers: mocks.computers,
  useComputerList: () => ({ data: { pages: [{ data: [], total: 0 }] } }),
}));
vi.mock("@/features/auth", () => ({ useLogout: () => vi.fn() }));
vi.mock("@/widgets/sidebar/SidebarAccountMenu", () => ({ SidebarAccountMenu: () => null }));

const quickNodes = [...topologyNodes, { ...topologyNodes[0], nodeId: "third", displayName: "Third Computer" }];

beforeEach(() => {
  mocks.computers.mockReturnValue({ quickNodes, total: 9, select: vi.fn() });
});
afterEach(cleanup);

async function show(locale = "en") {
  const i18n = createInstance();
  await i18n.init({
    lng: locale,
    resources: { [locale]: { computers: computerResources[locale as keyof typeof computerResources] } },
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
