// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { useAppStore } from "@/shared/store/app-store";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import { ComputerRegistrationActions } from "./ComputerRegistrationActions";

const mocks = vi.hoisted(() => ({ remove: vi.fn(), toggle: vi.fn(), deleted: vi.fn() }));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ computers: { delete: mocks.remove } }) }));
vi.mock("../hooks", () => ({ useProductScope: () => "fixture" }));
beforeEach(() => {
  vi.resetAllMocks();
  mocks.remove.mockResolvedValue({ nodeId: "manager-a" });
  useAppStore.setState({ selectedComputers: { fixture: "manager-a" } });
});
afterEach(cleanup);
async function show(locale = "zh-CN") {
  const i18n = createInstance();
  await i18n.init({ lng: locale, resources: { [locale]: { computers: computerResources[locale as keyof typeof computerResources], common: { cancel: locale === "en" ? "Cancel" : "取消" } } }, interpolation: { escapeValue: false } });
  return render(<QueryClientProvider client={new QueryClient()}><I18nextProvider i18n={i18n}><ComputerRegistrationActions node={topologyNodes[0]} pending={false} onToggleAccess={mocks.toggle} onDeleted={mocks.deleted} /></I18nextProvider></QueryClientProvider>);
}
it("keeps disable and remove together and cancel makes no API call", async () => {
  const { container } = await show();
  const actions = container.querySelector(".cmp-registration-actions")!;
  expect(within(actions as HTMLElement).getByRole("button", { name: "停用" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "停用" }));
  expect(mocks.toggle).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "移除注册" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("工作区文件均会保留");
  expect(screen.getByRole("button", { name: "取消" })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(mocks.remove).not.toHaveBeenCalled();
  expect(useAppStore.getState().selectedComputers.fixture).toBe("manager-a");
});
it("clears only the deleted execution selection after confirmed server success", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: "移除注册" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "移除注册" }));
  await waitFor(() => expect(mocks.deleted).toHaveBeenCalledOnce());
  expect(mocks.remove).toHaveBeenCalledExactlyOnceWith({ nodeId: "manager-a" });
  expect(useAppStore.getState().selectedComputers.fixture).toBe("");
});
it("keeps failed deletion visible and preserves registration selection", async () => {
  mocks.remove.mockRejectedValue(new TalosApiError({ code: AppProtocol.ErrorCode.ActiveTurnRunning, message: "Finish the active Turn first.", localization: null }));
  await show("en");
  fireEvent.click(screen.getByRole("button", { name: "Remove registration" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Remove registration" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Finish the active Turn first.");
  expect(mocks.deleted).not.toHaveBeenCalled();
  expect(useAppStore.getState().selectedComputers.fixture).toBe("manager-a");
});
it("disables both actions and cancellation while deletion is pending", async () => {
  let finish!: (value: { nodeId: string }) => void;
  mocks.remove.mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  await show();
  fireEvent.click(screen.getByRole("button", { name: "移除注册" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "移除注册" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "正在移除…" })).toBeDisabled());
  expect(screen.getByRole("button", { name: "停用" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
  finish({ nodeId: "manager-a" });
  await waitFor(() => expect(mocks.deleted).toHaveBeenCalledOnce());
});
