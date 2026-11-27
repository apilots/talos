// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import type { AppProtocol } from "@talos/sdk-core";
import { expect, it, vi } from "vitest";

import { i18n } from "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import { SessionDesktopShortcut } from "./SessionDesktopShortcut";
import { TaloLifecycleMenu } from "./talo-lifecycle";
import {
  listComputers, listFiles, query, QuickNodesProbe, readAutomation, readManagedWorker,
  resetManagedWorker, setupTaloConsoleTests, showConsole, startManagedWorker, taloInstance,
} from "./TaloConsole.test-support";

vi.mock("@/shared/api/desktop-client", () => ({ isDesktopRuntime: () => false, getDesktopBridge: () => undefined }));
setupTaloConsoleTests();

it("defaults collapsed without stealing focus, expands on demand, and restores focus on Escape", async () => {
  showConsole();
  expect(document.body).toHaveFocus();
  expect(screen.queryByRole("complementary", { name: "Manage Talo" })).not.toBeInTheDocument();

  const centerEmblem = await screen.findByRole("button", { name: "Talo · Talo is ready" });
  const moreTrigger = screen.getByRole("button", { name: "Open Talo controls" });
  expect(centerEmblem).toBeVisible();
  expect(moreTrigger).toBeVisible();

  fireEvent.click(centerEmblem);
  const panel = await screen.findByRole("complementary", { name: "Manage Talo" });
  expect(panel).toBeVisible();
  await waitFor(() => expect(within(panel).getByRole("button", { name: "Close Talo controls" })).toHaveFocus());
  expect(within(panel).getByText("4 vCPU · 8 GiB")).toBeVisible();
  expect(within(panel).getByText("Persistent Home (quota enforced)")).toBeVisible();
  expect(screen.queryByText(/Connect to the desktop to wake Talo/)).not.toBeInTheDocument();

  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("complementary", { name: "Manage Talo" })).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", { name: "Talo · Talo is ready" })).toHaveFocus());
});

it("wakes a sleeping Talo and opens the remote desktop in a modal without touching RightWorkspace tabs", async () => {
  const sleeping = { ...taloInstance, phase: "sleeping" as const };
  readManagedWorker.mockResolvedValue({ enabled: true, instance: sleeping });
  startManagedWorker.mockResolvedValue({ enabled: true, instance: taloInstance });

  showConsole(sleeping);
  const center = await screen.findByRole("button", { name: "Talo · Talo is asleep" });
  fireEvent.click(center);

  const panel = await screen.findByRole("complementary", { name: "Manage Talo" });
  expect(within(panel).getByRole("button", { name: "Wake Talo" })).toBeVisible();
  expect(within(panel).getByRole("button", { name: "Stop Talo" })).toBeVisible();

  fireEvent.click(within(panel).getByRole("button", { name: /Talo desktop/ }));
  expect(await screen.findByRole("dialog", { name: "Talo desktop" })).toBeVisible();
  expect(startManagedWorker).toHaveBeenCalledExactlyOnceWith({
    instanceId: taloInstance.instanceId,
    expectedGeneration: taloInstance.generation,
  });
  expect(useAppStore.getState().workspaceWindows).toHaveLength(0);
});

it("supports zh-CN localization and confirms reset and delete actions", async () => {
  await act(() => i18n.changeLanguage("zh-CN"));
  showConsole();

  fireEvent.click(await screen.findByRole("button", { name: "打开 Talo 控制台" }));
  const panel = await screen.findByRole("complementary", { name: "管理 Talo" });
  expect(within(panel).getByRole("tab", { name: "控制台" })).toHaveAttribute("aria-selected", "true");
  expect(within(panel).getByRole("tab", { name: "文件与产物" })).toBeVisible();
  expect(within(panel).getByText("计算规格")).toBeVisible();
  expect(within(panel).getByText("Home 已挂载（启用配额）")).toBeVisible();

  fireEvent.click(within(panel).getByRole("button", { name: "重置运行环境" }));
  const confirmReset = await screen.findByRole("alertdialog", { name: "重置 Talo？" });
  fireEvent.click(within(confirmReset).getByRole("button", { name: "重置 Talo" }));
  await waitFor(() => expect(resetManagedWorker).toHaveBeenCalledExactlyOnceWith({
    instanceId: taloInstance.instanceId,
    expectedGeneration: taloInstance.generation,
  }));
});

it("shows hover tips on action rows, configured idle sleep badge, automation count, and paginated files", async () => {
  const withoutDuration = { ...taloInstance, idleTimeoutSeconds: undefined };
  readManagedWorker.mockResolvedValue({ enabled: true, instance: withoutDuration });
  readAutomation.mockResolvedValue({
    goal: { id: "g-1", status: "active", objective: "Monitor build" },
    loops: [{ id: "l-1", status: "active", prompt: "Tick", createdAt: 1 }],
  });
  listFiles
    .mockResolvedValueOnce({
      data: [{ fileId: "f-1", originalFilename: "page-1.txt", mimeType: "text/plain" }],
      nextCursor: "cursor-2",
    })
    .mockResolvedValueOnce({
      data: [{ fileId: "f-2", originalFilename: "page-2.txt", mimeType: "text/plain" }],
      nextCursor: null,
    });

  const view = showConsole(withoutDuration);
  fireEvent.click(await screen.findByRole("button", { name: "Open Talo controls" }));
  const panel = await screen.findByRole("complementary", { name: "Manage Talo" });

  // Without backend duration, falls back cleanly without hardcoding 15m
  expect(within(panel).getByText("Auto-sleep")).toBeVisible();
  expect(panel).not.toHaveTextContent(/15m|15 分钟/);
  expect(await within(panel).findByText("2 running")).toBeVisible();

  // Hovering the action button itself does not pop a tooltip; hovering the ? trigger does
  fireEvent.mouseEnter(within(panel).getByRole("button", { name: "Reset runtime" }));
  expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  const resetTipBtn = within(panel).getByRole("button", { name: "About Reset runtime" });
  fireEvent.mouseEnter(resetTipBtn);
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Replaces Talo’s runtime while keeping Home files and settings.");
  fireEvent.mouseLeave(resetTipBtn);

  // Update with configured idleTimeoutSeconds=600 -> shows 10m dynamically
  readManagedWorker.mockResolvedValue({ enabled: true, instance: taloInstance });
  await act(async () => { await query.invalidateQueries({ queryKey: ["talos"] }); });
  expect(await within(panel).findByText("Sleep after 10m idle")).toBeVisible();
  fireEvent.focus(within(panel).getByRole("button", { name: "Auto-sleep details" }));
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Automatically sleeps after 10m without chat or desktop input");

  // Files tab supports cursor pagination
  fireEvent.click(within(panel).getByRole("tab", { name: "Files" }));
  expect(await within(panel).findByRole("button", { name: "page-1.txt" })).toBeVisible();
  fireEvent.click(within(panel).getByRole("button", { name: "Load more files" }));
  expect(await within(panel).findByRole("button", { name: "page-2.txt" })).toBeVisible();
  expect(listFiles).toHaveBeenNthCalledWith(2, { cursor: "cursor-2", limit: 50 });
  view.unmount();
});

it("resolves TaloLifecycleMenu portal inside its enclosing dialog even when instance loads asynchronously", async () => {
  let resolveWorker!: (value: { enabled: boolean; instance: AppProtocol.ManagedWorkerInfo }) => void;
  readManagedWorker.mockImplementationOnce(() => new Promise((resolve) => {
    resolveWorker = resolve;
  }));

  render(
    <QueryClientProvider client={query}>
      <MemoryRouter>
        <div role="dialog" data-testid="enclosing-dialog">
          <TaloLifecycleMenu desktopNodeId={taloInstance.nodeId} />
        </div>
      </MemoryRouter>
    </QueryClientProvider>,
  );

  expect(screen.queryByRole("button", { name: "Manage Talo" })).not.toBeInTheDocument();
  await waitFor(() => expect(resolveWorker).toBeTypeOf("function"));
  await act(() => {
    resolveWorker({ enabled: true, instance: taloInstance });
   return Promise.resolve(); });

  const menuTrigger = await screen.findByRole("button", { name: "Manage Talo" });
  fireEvent.keyDown(menuTrigger, { key: "Enter" });
  const menu = await screen.findByRole("menu");
  expect(screen.getByTestId("enclosing-dialog")).toContainElement(menu);
});

it("excludes managed Talo nodes from quickNodes, computer lists, and selectedComputerId", async () => {
  listComputers.mockResolvedValue({
    data: [
      { ...topologyNodes[0], nodeId: taloInstance.nodeId, displayName: "Talo", managedWorkerPhase: "ready" },
      ...topologyNodes,
    ],
    total: 3,
    nextCursor: null,
  });
  const scope = JSON.stringify(["http://localhost:3000", "alice"]);
  useAppStore.setState({ selectedComputers: { [scope]: taloInstance.nodeId } });
  render(
    <QueryClientProvider client={query}>
      <MemoryRouter>
        <QuickNodesProbe />
        <SessionDesktopShortcut nodeId={taloInstance.nodeId} />
      </MemoryRouter>
    </QueryClientProvider>,
  );

  await waitFor(() => expect(screen.getByLabelText("Quick nodes")).toHaveTextContent("manager-a,worker-b"));
  expect(screen.getByLabelText("Quick nodes").textContent).not.toContain("talo-node");
  expect(screen.getByLabelText("Listed nodes")).toHaveTextContent("manager-a,worker-b");
  expect(screen.getByLabelText("Listed total")).toHaveTextContent("2");
  await waitFor(() => expect(screen.getByLabelText("Selected node")).toHaveTextContent("none"));
  expect(screen.queryByRole("button", { name: /View computer|Open browser/i })).not.toBeInTheDocument();
});
