// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, expect, it, vi } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import { computerResources } from "@/shared/i18n/computerResources";
import { ComputerGraph } from "./ComputerGraph";
import { ComputerTopologyList } from "./ComputerTopologyList";
import { SyncPeerDetails } from "./SyncPeerDetails";
import {
  topologyMetrics,
  topologyNodes,
} from "@/shared/test/computer-topology.fixture";

afterEach(cleanup);

it.each(["en", "zh-CN"])(
  "renders only observed sync mappings with bidirectional arrows (%s)",
  async (locale) => {
    const i18n = createInstance();
    await i18n.init({
      lng: locale,
      resources: {
        [locale]: {
          computers:
            computerResources[locale as keyof typeof computerResources],
        },
      },
      interpolation: { escapeValue: false },
    });
    const inspect = vi.fn();
    const { container, rerender } = render(
      <I18nextProvider i18n={i18n}>
        <ComputerGraph
          nodes={topologyNodes}
          metrics={topologyMetrics}
          controlPlane="https://control.example"
          layer="all"
          onInspect={inspect}
          onService={vi.fn()}
        />
      </I18nextProvider>,
    );
    expect(container.querySelectorAll('[data-flow="execution"]')).toHaveLength(
      2,
    );
    expect(screen.queryByText(computerResources[locale as keyof typeof computerResources].lastKnownEndpointShort)).toBeNull();
    const sync = container.querySelector(
      '[data-flow="sync"][data-computer-id="manager-a"]',
    );
    expect(sync).toHaveAttribute("marker-start");
    expect(sync).toHaveAttribute("marker-end");
    expect(
      container.querySelector(
        '[data-flow="sync"][data-computer-id="worker-b"]',
      ),
    ).toBeNull();
    fireEvent.click(
      screen.getByRole("button", {
        name:
          locale === "en" ? "Inspect Build Worker" : "查看 Build Worker 的详情",
      }),
    );
    expect(inspect).toHaveBeenCalledWith("worker-b");
    rerender(
      <I18nextProvider i18n={i18n}>
        <ComputerGraph
          nodes={topologyNodes}
          controlPlane="https://control.example"
          layer="sync"
          onInspect={inspect}
          onService={vi.fn()}
        />
      </I18nextProvider>,
    );
    expect(container.querySelectorAll('[data-flow="sync"]')).toHaveLength(0);
    expect(container.querySelectorAll('[data-flow="execution"]')).toHaveLength(
      0,
    );
    expect(container.querySelectorAll(".cmp-device")).toHaveLength(2);
    expect(
      screen.getAllByText(locale === "en" ? "Sync unknown" : "同步状态未知"),
    ).toHaveLength(2);
  },
);

it("keeps the Control Plane edge unknown until its exact peer is observed", async () => {
  const i18n = createInstance();
  await i18n.init({
    lng: "en",
    resources: { en: { computers: computerResources.en } },
  });
  const metrics = { ...topologyMetrics, controlPlanePeerId: "peer-control" };
  const show = () => (
    <I18nextProvider i18n={i18n}>
      <ComputerGraph
        nodes={topologyNodes}
        metrics={metrics}
        controlPlane="https://control.example"
        layer="all"
        onInspect={vi.fn()}
        onService={vi.fn()}
      />
    </I18nextProvider>
  );
  const { container, rerender } = render(show());
  expect(container.querySelector('[data-service="controlPlane"]')).toHaveAttribute("data-sync-state", "unknown");
  metrics.peers = [
    ...metrics.peers,
    { ...metrics.peers[0], peerId: "peer-control", computerIds: [] },
  ];
  rerender(show());
  expect(
    container.querySelector('[data-flow="sync"][data-service="controlPlane"]'),
  ).not.toBeNull();
});

it.each(["en", "zh-CN"])("shows enum states on both paths and keeps execution unchanged (%s)", async (locale) => {
  const i18n = createInstance();
  await i18n.init({ lng: locale, resources: { [locale]: { computers: computerResources[locale as keyof typeof computerResources] } } });
  const now = topologyMetrics.observedAt;
  const peer: AppProtocol.SyncPeerMetrics = { ...topologyMetrics.peers[0], lastActivityAt: null, lastHeartbeatAt: now };
  const metrics = { ...topologyMetrics, controlPlanePeerId: peer.peerId, peers: [peer] };
  const status: AppProtocol.SyncStatusResponse = { configuredEnabled: true, connection: "connected" as const, peerId: peer.peerId,
    pendingChanges: 0, lastAcknowledgedAt: null, lastError: null,
    localComputerId: "manager-a", canClaimLocalHistory: false };
  const show = (syncStatus = status) => <I18nextProvider i18n={i18n}>
    <ComputerGraph nodes={topologyNodes} metrics={metrics} syncStatus={syncStatus}
      controlPlane="https://control.example" layer="all" onInspect={vi.fn()} onService={vi.fn()} />
  </I18nextProvider>;
  const { container, rerender } = render(show());
  const states = () => [...container.querySelectorAll('[data-flow="sync"]')].map((wire) => wire.getAttribute("data-sync-state"));
  expect(states()).toEqual(["idle", "idle"]);
  expect(screen.getAllByText(locale === "en" ? "Online · idle" : "在线 · 空闲").length).toBeGreaterThan(0);
  peer.lastActivityAt = now;
  rerender(show());
  expect(states()).toEqual(["syncing", "syncing"]);
  peer.lastActivityAt = now - 60;
  rerender(show());
  expect(states()).toEqual(["idle", "idle"]);
  rerender(show({ ...status, connection: "disconnected" }));
  expect(states()).toEqual(["disconnected", "disconnected"]);
  rerender(show());
  expect(states()).toEqual(["idle", "idle"]);
  expect(container.querySelectorAll('[data-flow="execution"]')).toHaveLength(2);
  expect(container.querySelectorAll('[data-flow="execution"].is-offline')).toHaveLength(1);
});

it("renders the same local conflict in the graph, list and details without attributing it to a Worker", async ({ expect }) => {
  const i18n = createInstance();
  await i18n.init({ lng: "zh-CN", resources: { "zh-CN": { computers: computerResources["zh-CN"] } } });
  const status: AppProtocol.SyncStatusResponse = { configuredEnabled: true, connection: "connected", peerId: "peer-a",
    pendingChanges: 2, lastAcknowledgedAt: null, lastError: { code: "conflict", message: "Conflict", localization: null },
    localComputerId: "manager-a", canClaimLocalHistory: false };
  const { container } = render(<I18nextProvider i18n={i18n}>
    <ComputerGraph nodes={topologyNodes} metrics={topologyMetrics} syncStatus={status}
      controlPlane="https://control.example" layer="all" onInspect={vi.fn()} onService={vi.fn()} />
    <ComputerTopologyList nodes={topologyNodes} metrics={topologyMetrics} syncStatus={status} onInspect={vi.fn()} />
    <SyncPeerDetails peers={topologyMetrics.peers} metrics={topologyMetrics} syncStatus={status} />
  </I18nextProvider>);
  expect(screen.getAllByText("同步冲突")).toHaveLength(3);
  expect(screen.getByText("2 项待同步")).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText("最近同步心跳")).toBeVisible();
  expect(container.querySelector('[data-computer-id="worker-b"][data-flow="sync"]')).toBeNull();
  expect([...container.querySelectorAll('[data-flow="sync"]')].map((wire) => wire.getAttribute("data-sync-state")))
    .toEqual(["conflict"]);
});
