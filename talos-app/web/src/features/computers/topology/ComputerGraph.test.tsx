// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import { computerResources } from "@/shared/i18n/computerResources";
import { ComputerGraph } from "./ComputerGraph";
import { ComputerTopologyList } from "./ComputerTopologyList";
import { SyncPeerDetails } from "./SyncPeerDetails";
import { ComputerServiceDetails } from "./ComputerServiceDetails";
import {
  topologyMetrics,
  topologyNodes,
} from "@/shared/test/computer-topology.fixture";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(topologyMetrics.observedAt * 1000); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

it.each(["starting", "restarting", "stopped", "error", "ready", "unknown"] as const)(
  "renders local %s separately from remote connectivity in graph and list", async (kind) => {
    const i18n = createInstance();
    await i18n.init({ lng: "en", resources: { en: { computers: computerResources.en } } });
    const runtime = kind === "unknown" ? undefined : { kind, message: null };
    const local = { ...topologyNodes[0], available: false, status: "disabled" as const };
    const { container } = render(<I18nextProvider i18n={i18n}>
      <ComputerGraph nodes={[local]} localId={local.nodeId} localRuntime={runtime}
        directoryAvailable={false} controlPlane="https://control.example" layer="all" onInspect={vi.fn()} onService={vi.fn()} />
      <ComputerTopologyList nodes={[local]} localId={local.nodeId} localRuntime={runtime} onInspect={vi.fn()} />
    </I18nextProvider>);
    expect(screen.getAllByText(`${computerResources.en.localRuntime[kind]} · ${computerResources.en.remoteDisabled}`)).toHaveLength(2);
    expect(container.querySelectorAll('[data-flow="execution"]')).toHaveLength(0);
    expect(container.querySelectorAll(".cmp-device .cmp-dot.is-online, .cmp-inline-status .cmp-dot.is-online")).toHaveLength(kind === "ready" ? 2 : 0);
  },
);

it.each(["en", "zh-CN"])("shows disabled Sync instead of unknown without metrics (%s)", async (locale) => {
  const i18n = createInstance();
  const resources = computerResources[locale as keyof typeof computerResources];
  await i18n.init({ lng: locale, resources: { [locale]: { computers: resources } } });
  const syncStatus: AppProtocol.SyncStatusResponse = {
    configuredEnabled: false, connection: "disabled", peerId: "peer-control",
    pendingChanges: 0, lastAcknowledgedAt: null, lastError: null,
    localComputerId: "manager-a", canClaimLocalHistory: false,
  };
  const { container } = render(<I18nextProvider i18n={i18n}>
    <ComputerGraph nodes={topologyNodes} syncStatus={syncStatus}
      controlPlane="https://control.example" layer="all" onInspect={vi.fn()} onService={vi.fn()} />
    <section data-testid="disabled-sync-details">
      <ComputerServiceDetails service="sync" syncStatus={syncStatus}
        syncEndpoint="wss://previous.example/sync" controlPlane="https://control.example" />
    </section>
  </I18nextProvider>);
  const service = screen.getByRole("button", { name: locale === "en" ? "Inspect Talos Sync" : "查看 Talos Sync 的详情" });
  expect(service).toHaveTextContent(resources.syncDisabled);
  expect(service).not.toHaveTextContent(resources.unknown);
  expect(screen.queryByText(resources.controlPlaneSyncUnknown)).toBeNull();
  expect(screen.getByTestId("disabled-sync-details").querySelector("dd")).toHaveTextContent(resources.syncDisabled);
  expect(screen.queryByText("wss://previous.example/sync")).toBeNull();
  expect([...container.querySelectorAll('[data-flow="sync"]')].map((wire) => wire.getAttribute("data-sync-state"))).toEqual(["disabled"]);
});

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
  expect(screen.getAllByText("待上传: 2")).toHaveLength(3);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText("最近同步心跳")).toBeVisible();
  expect(container.querySelector('[data-computer-id="worker-b"][data-flow="sync"]')).toBeNull();
  expect([...container.querySelectorAll('[data-flow="sync"]')].map((wire) => wire.getAttribute("data-sync-state")))
    .toEqual(["conflict"]);
});

it.each(["en", "zh-CN"])("shares current progress and expiry across graph, list and details (%s)", async (locale) => {
  const i18n = createInstance();
  const resources = computerResources[locale as keyof typeof computerResources];
  await i18n.init({ lng: locale, resources: { [locale]: { computers: resources } } });
  const peer: AppProtocol.SyncPeerMetrics = { ...topologyMetrics.peers[0], lastActivityAt: null,
    progress: { state: "caughtUp", downloadLagMs: 0, pendingUploads: 5, observedAt: topologyMetrics.observedAt - 20,
      lastAppliedAt: topologyMetrics.observedAt - 60,
      sourceDelays: [{ sourcePeerId: "source-b", observedDelayMs: 321 }] } };
  let metrics = { ...topologyMetrics, peers: [peer] };
  const show = () => <I18nextProvider i18n={i18n}>
    <section data-testid="graph"><ComputerGraph nodes={topologyNodes.slice(0, 1)} metrics={metrics}
      controlPlane="https://control.example" layer="all" onInspect={vi.fn()} onService={vi.fn()} /></section>
    <section data-testid="list"><ComputerTopologyList nodes={topologyNodes.slice(0, 1)} metrics={metrics} onInspect={vi.fn()} /></section>
    <section data-testid="details"><SyncPeerDetails peers={[peer]} metrics={metrics} /></section>
  </I18nextProvider>;
  const { container, rerender } = render(show());
  for (const id of ["graph", "list", "details"]) {
    const view = within(screen.getByTestId(id));
    expect(view.getByText(resources.syncIdle)).toBeVisible();
    expect(view.getByText(`${resources.replicationProgress}: ${resources.syncCaughtUp} · ${locale === "en" ? "0 ms" : "0毫秒"}`)).toBeVisible();
    expect(view.getByText(`${resources.pendingUploads}: 5`)).toBeVisible();
  }
  expect(container.querySelector('[data-flow="sync"]')).toHaveAttribute("data-sync-state", "idle");
  expect(screen.getByText(resources.sourceDelayHelp)).toBeVisible();
  expect(screen.getByText(locale === "en" ? "321 ms" : "321毫秒")).toBeVisible();
  peer.progress!.state = "behind";
  peer.progress!.downloadLagMs = 1400;
  rerender(show());
  expect(container.querySelectorAll('[data-download-state="behind"]')).toHaveLength(3);
  expect(container.querySelector('[data-flow="sync"]')).toHaveAttribute("data-sync-state", "idle");
  peer.progress!.state = "recovering";
  rerender(show());
  expect(container.querySelectorAll('[data-download-state="recovering"]')).toHaveLength(3);
  expect(container.querySelector('[data-flow="sync"]')).toHaveAttribute("data-sync-state", "idle");
  peer.progress!.state = "caughtUp";
  metrics = { ...metrics, observedAt: topologyMetrics.observedAt - 16 };
  rerender(show());
  expect(container.querySelectorAll('[data-download-state="unknown"]')).toHaveLength(3);
  expect(container.querySelector('[data-flow="sync"]')).toHaveAttribute("data-sync-state", "unknown");
  expect(screen.queryByText(locale === "en" ? "321 ms" : "321毫秒")).toBeNull();
  expect(screen.queryByText(resources.sourceDelayHelp)).toBeNull();
  expect(screen.getByText(resources.lastSyncHeartbeat)).toBeVisible();
  expect(screen.getByRole("region", { name: resources.syncTitle })).toHaveAttribute("title", resources.observationsExpired);
  for (const id of ["graph", "list", "details"]) {
    expect(within(screen.getByTestId(id)).getByText(`${resources.pendingUploads}: N/A`)).toBeVisible();
  }
});
