// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, useLocation } from "react-router-dom";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/shared/i18n/setup";
import { resetTalosClientFactory, setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { connectionScope } from "@/shared/store/connection-scope";
import { useAppStore } from "@/shared/store/app-store";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import { ComputerHomePage } from "./ComputerHomePage";

vi.mock("@/shared/api/desktop-client", () => ({ getDesktopBridge: () => undefined }));
const list = vi.fn();
const query = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });

function Location() { return <output aria-label="Location">{useLocation().pathname}</output>; }
function show() {
  return render(<StrictMode><QueryClientProvider client={query}><MemoryRouter initialEntries={["/files"]}><ComputerHomePage /><Location /></MemoryRouter></QueryClientProvider></StrictMode>);
}

beforeEach(() => {
  void i18n.changeLanguage("en");
  list.mockReset().mockResolvedValue({ data: topologyNodes, total: 2, nextCursor: null });
  useAppStore.setState({ selectedComputers: {}, connection: { ...useAppStore.getInitialState().connection, authStatus: "authenticated", userId: "alice" } });
  setTalosClientFactory(() => ({ computers: { list, read: async ({ nodeId }: { nodeId: string }) => topologyNodes.find(node => node.nodeId === nodeId) }, sync: { status: async () => ({ localComputerId: null }) } }) as unknown as TalosWebClient);
  HTMLDialogElement.prototype.showModal = function () { this.open = true; this.querySelector<HTMLElement>("button")?.focus(); };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(() => { cleanup(); query.clear(); resetTalosClientFactory(); });

it("uses the existing computer rows and selects even an offline computer without changing its status", async () => {
  show();
  expect(await screen.findByRole("heading", { name: "Choose a computer" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Studio · Connected" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Build Worker · Offline" }));
  expect(useAppStore.getState().selectedComputers[connectionScope(useAppStore.getState().connection)]).toBe("worker-b");
  expect(screen.getByLabelText("Location")).toHaveTextContent("/");
});

it.each(["Close", "Got it", "Escape"])("shows first-computer guidance only on demand and returns focus after %s", async action => {
  list.mockResolvedValue({ data: [], total: 0, nextCursor: null });
  show();
  expect(await screen.findByRole("heading", { name: "Connect your first computer" })).toBeVisible();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(document.body).toHaveFocus();
  const trigger = screen.getByRole("button", { name: "Connect a computer" });
  trigger.focus();
  fireEvent.click(trigger);
  const dialog = screen.getByRole("dialog", { name: "Connect a computer" });
  expect(dialog).toHaveTextContent("then sign in with the same account");
  if (action === "Escape") fireEvent(dialog, new Event("cancel", { cancelable: true }));
  else fireEvent.click(within(dialog).getByRole("button", { name: action }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

it("does not confuse directory loading or failure with zero registered computers and supports retry", async () => {
  let reject!: (reason: Error) => void;
  list.mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail; }));
  show();
  expect(within(screen.getByRole("region", { name: "Home" })).getByRole("status")).toHaveTextContent("Loading");
  expect(screen.queryByText("Connect your first computer")).not.toBeInTheDocument();
  await act(async () => reject(new TalosApiError({ code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Directory unavailable.", localization: null })));
  expect(await screen.findByRole("alert")).toHaveTextContent("Directory unavailable.");
  expect(screen.queryByText("Connect your first computer")).not.toBeInTheDocument();
  list.mockResolvedValue({ data: [], total: 0, nextCursor: null });
  fireEvent.click(screen.getByRole("button", { name: "Retry now" }));
  expect(await screen.findByRole("heading", { name: "Connect your first computer" })).toBeVisible();
});

it("searches beyond the initial computer list without treating no matches as an unregistered account", async () => {
  const more = { ...topologyNodes[0], nodeId: "remote", displayName: "Remote office" };
  list.mockImplementation(async ({ query: search }) => search
    ? { data: search === "office" ? [more] : [], total: search === "office" ? 1 : 0, nextCursor: null }
    : { data: topologyNodes, total: 21, nextCursor: "next" });
  show();
  const search = await screen.findByRole("searchbox", { name: "Search computers" });
  expect(screen.queryByRole("button", { name: "Show more" })).not.toBeInTheDocument();
  fireEvent.change(search, { target: { value: "office" } });
  await waitFor(() => expect(list).toHaveBeenCalledWith(expect.objectContaining({ query: "office" })));
  expect(await screen.findByRole("button", { name: "Remote office · Connected" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Studio · Connected" })).not.toBeInTheDocument();
  fireEvent.change(search, { target: { value: "no-match" } });
  expect(await screen.findByText("No computers found")).toBeVisible();
  expect(screen.getByRole("heading", { name: "Choose a computer" })).toBeVisible();
  expect(screen.queryByText("Connect your first computer")).not.toBeInTheDocument();
});

it("translates the first-computer prompt and connection guidance", async () => {
  await i18n.changeLanguage("zh-CN");
  list.mockResolvedValue({ data: [], total: 0, nextCursor: null });
  show();
  expect(await screen.findByRole("heading", { name: "连接你的第一台电脑" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "连接 Computer" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("然后登录同一账号");
});
