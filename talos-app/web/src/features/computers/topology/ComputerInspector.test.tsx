// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import type { ComponentProps, ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { ComputerInspector } from "./ComputerInspector";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import { useAppStore } from "@/shared/store/app-store";

const mocks = vi.hoisted(() => ({ read: vi.fn(), retry: vi.fn(), rename: vi.fn(), desktopButton: vi.fn<(props: { nodeId?: string; online: boolean; shortcut?: boolean }) => ReactNode>(() => null) }));
vi.mock("../hooks", () => ({ useComputer: mocks.read, useProductScope: () => "fixture" }));
vi.mock("@/shared/api/desktop-client", () => ({ getDesktopBridge: () => undefined }));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ computers: { rename: mocks.rename } }) }));
vi.mock("../ComputerDesktop", () => ({
  inspectorActionButtonClassName: "h-9 min-w-0 gap-1.5 rounded-lg bg-selected px-2.5 py-0 text-[13px] leading-5 whitespace-nowrap enabled:hover:bg-ink/[0.09]",
  ComputerDesktopButton: (props: { nodeId?: string; online: boolean; shortcut?: boolean }) => mocks.desktopButton(props),
}));

const node = topologyNodes[1]!;
beforeEach(() => {
  vi.clearAllMocks();
  mocks.desktopButton.mockImplementation(() => null);
  mocks.read.mockReturnValue({ data: node, error: null, refetch: mocks.retry });
  useAppStore.setState({ connection: { ...useAppStore.getInitialState().connection, userId: "alice" } });
});
afterEach(cleanup);

async function show(locale = "en", props: Partial<ComponentProps<typeof ComputerInspector>> = {}) {
  const i18n = createInstance();
  await i18n.init({ lng: locale, interpolation: { escapeValue: false }, resources: {
    [locale]: { computers: computerResources[locale as keyof typeof computerResources],
      common: { loading: "Loading", save: "Save", retryNow: locale === "en" ? "Retry" : "立即重试" } },
  } });
  const queries = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queries.setQueryData(["talos", "fixture", "computers", ""], {
    pages: [{ data: [node], total: 1 }], pageParams: [null],
  });
  const onOpen = vi.fn();
  const result = render(<QueryClientProvider client={queries}><I18nextProvider i18n={i18n}>
    <ComputerInspector nodeId={node.nodeId} localId="manager-a" controlPlane="https://server.example" onOpen={onOpen} onDeleted={vi.fn()} {...props} />
  </I18nextProvider></QueryClientProvider>);
  return { ...result, onOpen };
}

it("opens only the successfully loaded Computer and allows renaming", async () => {
  const { onOpen } = await show();
  expect(screen.getByRole("heading", { name: node.displayName })).toBeVisible();
  expect(onOpen).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Open computer" }));
  expect(onOpen).toHaveBeenCalledExactlyOnceWith(node.nodeId);
  fireEvent.change(screen.getByRole("textbox", { name: "Rename" }), { target: { value: "Renamed worker" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(mocks.rename).toHaveBeenCalledExactlyOnceWith({ nodeId: node.nodeId, displayName: "Renamed worker" }));
});

it("renders listed metadata while detail is pending without granting management actions", async () => {
  mocks.read.mockReturnValue({ data: node, error: null, isPlaceholderData: true, refetch: mocks.retry });
  await show("en", { listedNode: node });
  expect(mocks.read).toHaveBeenCalledWith(node.nodeId, node);
  expect(screen.getByRole("heading", { name: node.displayName })).toBeVisible();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Disable remote access" })).not.toBeInTheDocument();
});

it("shows loading only when neither a detail nor a listed row is available", async () => {
  mocks.read.mockReturnValue({ data: undefined, error: null, refetch: mocks.retry });
  await show();
  expect(screen.getByRole("status")).toHaveTextContent("Loading");
  expect(screen.queryByRole("heading")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open computer" })).not.toBeInTheDocument();
});

it.for(["en", "zh-CN"])("withdraws remote detail and shows a localized retry alert on failure (%s)", async (locale, { expect }) => {
  mocks.read.mockReturnValue({ data: undefined, error: new TalosApiError({
    code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Transport diagnostics", localization: null,
  }), refetch: mocks.retry });
  await show(locale);
  const labels = computerResources[locale as keyof typeof computerResources];
  const alert = screen.getByRole("alert");
  expect(screen.queryByRole("button", { name: labels.open })).not.toBeInTheDocument();
  expect(alert).toHaveClass("text-danger");
  expect(alert).toHaveTextContent(labels.networkError);
  expect(alert).not.toHaveTextContent("Transport diagnostics");
  expect(screen.queryByRole("heading")).not.toBeInTheDocument();
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: labels.disable })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: locale === "en" ? "Retry" : "立即重试" }));
  expect(mocks.retry).toHaveBeenCalledOnce();
});

it("preserves an authoritative not-found error instead of calling it a network failure", async () => {
  mocks.read.mockReturnValue({ data: undefined, error: new TalosApiError({
    code: AppProtocol.ErrorCode.ResourceNotFound, message: "Computer not found.", localization: null,
  }), refetch: mocks.retry });
  await show();
  expect(screen.getByRole("alert")).toHaveTextContent("Computer not found.");
  expect(screen.getByRole("alert")).not.toHaveTextContent(computerResources.en.networkError);
});

it.each(["en", "zh-CN"])("keeps local hardware and runtime visible without granting directory management (%s)", async (locale) => {
  mocks.read.mockReturnValue({ data: { ...node, ownerUserId: "alice" }, error: new TypeError("Failed to fetch"), refetch: mocks.retry });
  const local = { ...topologyNodes[0]!, nodeId: node.nodeId, displayName: "Local hardware", ownerUserId: "alice",
    systemInfo: { os: "macOs" as const, osVersion: "macOS", architecture: "aarch64", hostname: "local-host",
      cpuModel: "Local CPU", logicalCpuCount: 8, memoryBytes: 16 * 1024 ** 3, boardModel: null,
      graphics: [], networkInterfaces: [], workerVersion: "0.1.19" },
  };
  const { onOpen } = await show(locale, { localId: node.nodeId, localNode: local, localRuntime: { kind: "ready", message: null } });
  const labels = computerResources[locale as keyof typeof computerResources];
  expect(screen.getByRole("heading", { name: local.displayName })).toBeVisible();
  expect(screen.getByText(labels.localRuntime.ready)).toBeVisible();
  expect(screen.getByText(local.systemInfo!.hostname)).toBeVisible();
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: labels.disable })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: labels.open }));
  expect(onOpen).toHaveBeenCalledExactlyOnceWith(node.nodeId);
});

it("retains directory ownership and access status over a synthetic local row", async () => {
  const local = { ...node, displayName: "This computer", mode: "embedded" as const, ownerUserId: null, status: "enabled" as const };
  mocks.read.mockReturnValue({ data: { ...node, displayName: "Registered local", status: "disabled" }, error: null, refetch: mocks.retry });
  await show("en", { localId: node.nodeId, localNode: local, localRuntime: { kind: "ready", message: null } });
  expect(screen.getByRole("heading", { name: "Registered local" })).toBeVisible();
  expect(screen.getByRole("textbox", { name: "Rename" })).toBeVisible();
  expect(screen.getByText(/Ready · Remote access disabled/)).toBeVisible();
});

it("disables retry while a detail retry is pending", async () => {
  mocks.read.mockReturnValue({ data: undefined, error: new TypeError("Failed to fetch"), isFetching: true, refetch: mocks.retry });
  await show();
  expect(screen.getByRole("button", { name: "Retry" })).toBeDisabled();
});

it("places primary and secondary inspector actions inside a single action bar", async () => {
  const { container, unmount } = await show();
  const actionBar = container.querySelector(".cmp-inspector-actions");
  expect(actionBar).not.toBeNull();
  const openButton = screen.getByRole("button", { name: "Open computer" });
  expect(openButton.parentElement).toBe(actionBar);
  expect(openButton).toHaveClass("border", "border-border", "bg-selected");
  expect(openButton.querySelector(".lucide-laptop")).not.toBeNull();
  expect(actionBar?.querySelectorAll("button")).toHaveLength(1);
  unmount();

  mocks.desktopButton.mockImplementation(() => <button type="button">Desktop</button>);
  const second = await show();
  const secondActionBar = second.container.querySelector(".cmp-inspector-actions");
  expect(secondActionBar).not.toBeNull();
  const buttons = secondActionBar?.querySelectorAll("button");
  expect(buttons).toHaveLength(2);
  expect(buttons?.[0]).toHaveTextContent("Open computer");
  expect(buttons?.[1]).toHaveTextContent("Desktop");
  expect(mocks.desktopButton).toHaveBeenLastCalledWith({ nodeId: node.nodeId, online: node.available });
});
