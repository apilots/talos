// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyMetrics } from "@/shared/test/computer-topology.fixture";
import { SyncPeerDetails } from "./SyncPeerDetails";
import { ComputerServiceDetails } from "./ComputerServiceDetails";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(topologyMetrics.observedAt * 1000); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

it.each(["en", "zh-CN"])("shows current stored resources independently from the machine directory (%s)", async (locale) => {
  const i18n = createInstance();
  const resources = computerResources[locale as keyof typeof computerResources];
  await i18n.init({ lng: locale, resources: { [locale]: { computers: resources } } });
  const { container, rerender } = render(<I18nextProvider i18n={i18n}><ComputerServiceDetails service="sync"
    metrics={topologyMetrics} controlPlane="https://control.example" /></I18nextProvider>);
  expect(container.querySelectorAll(".cmp-facts dt")).toHaveLength(4);
  expect(screen.getByText(resources.syncProjects)).toBeVisible();
  expect(screen.getByText(resources.syncSessions)).toBeVisible();
  expect(screen.getByText(resources.syncFiles)).toBeVisible();
  rerender(<I18nextProvider i18n={i18n}><ComputerServiceDetails service="sync"
    metrics={{ ...topologyMetrics, observedAt: topologyMetrics.observedAt - 16 }} controlPlane="https://control.example" /></I18nextProvider>);
  expect([...container.querySelectorAll(".cmp-facts dd")].slice(1).map((row) => row.textContent)).toEqual(["—", "—", "—"]);
});

it.each(["en", "zh-CN"])("keeps numeric download lag separate from relative heartbeat age (%s)", async (locale) => {
  const i18n = createInstance();
  await i18n.init({ lng: locale, resources: { [locale]: { computers: computerResources[locale as keyof typeof computerResources] } } });
  const peer: AppProtocol.SyncPeerMetrics = { ...topologyMetrics.peers[0],
    lastHeartbeatAt: topologyMetrics.observedAt - 8,
    progress: { state: "behind", downloadLagMs: 7_200_000, pendingUploads: 0,
      observedAt: topologyMetrics.observedAt, lastAppliedAt: null, sourceDelays: [] } };
  render(<I18nextProvider i18n={i18n}><SyncPeerDetails peers={[peer]}
    metrics={{ ...topologyMetrics, peers: [peer] }} /></I18nextProvider>);
  expect(screen.getByText(locale === "en" ? "Hub download: Behind by 2 hr" : "Hub 下载: 落后 2小时")).toBeVisible();
  const heartbeat = screen.getByText(locale === "en" ? "8 sec ago" : "8秒前");
  expect(heartbeat.tagName).toBe("TIME");
  expect(heartbeat).toHaveAttribute("datetime", new Date((topologyMetrics.observedAt - 8) * 1000).toISOString());
});

it.each(["en", "zh-CN"])("keeps the same sync detail fields across unavailable states (%s)", async (locale) => {
  const i18n = createInstance();
  const resources = computerResources[locale as keyof typeof computerResources];
  await i18n.init({ lng: locale, resources: { [locale]: { computers: resources } } });
  const peer = topologyMetrics.peers[0];
  const status: AppProtocol.SyncStatusResponse = { configuredEnabled: true, connection: "connected", peerId: peer.peerId,
    localComputerId: "manager-a", pendingChanges: 2, lastAcknowledgedAt: null, lastError: null, canClaimLocalHistory: false };
  const show = (peers: AppProtocol.SyncPeerMetrics[], metrics?: AppProtocol.SyncMetricsResponse, syncStatus?: AppProtocol.SyncStatusResponse) =>
    <I18nextProvider i18n={i18n}><SyncPeerDetails peers={peers} metrics={metrics} syncStatus={syncStatus} /></I18nextProvider>;
  const { container, rerender } = render(show([peer], topologyMetrics));
  const labels = () => [...container.querySelectorAll(".cmp-peer dt")].map((row) => row.textContent);
  const originalLabels = labels();
  const field = (label: string) => screen.getByText(label, { selector: "dt" }).nextElementSibling;
  expect(originalLabels).toHaveLength(9);
  const cases = [
    { peers: [], metrics: undefined, status: undefined, state: "unknown" },
    { peers: [peer], metrics: { ...topologyMetrics, observedAt: topologyMetrics.observedAt - 16 }, status: undefined, state: "unknown" },
    { peers: [{ ...peer, connections: 0 }], metrics: topologyMetrics, status: { ...status, connection: "disconnected" as const }, state: "disconnected" },
    { peers: [peer], metrics: topologyMetrics, status: { ...status, connection: "disconnected" as const }, state: "disconnected" },
    { peers: [], metrics: undefined, status: { ...status, configuredEnabled: false, connection: "disabled" as const }, state: "disabled" },
    { peers: [], metrics: undefined, status: { ...status, lastError: { code: "serviceUnavailable" as const, message: "Unavailable", localization: null } }, state: "error" },
  ];
  for (const scenario of cases) {
    rerender(show(scenario.peers, scenario.metrics, scenario.status));
    expect(labels()).toEqual(originalLabels);
    expect(container.querySelector("[data-sync-state]")).toHaveAttribute("data-sync-state", scenario.state);
    expect(field(resources.inboundRate)).toHaveTextContent("N/A");
    expect(field(resources.updateRate)).toHaveTextContent("N/A");
    expect(field(resources.peerIdentity)).toHaveTextContent(scenario.peers[0]?.peerId ?? scenario.status?.peerId ?? "N/A");
    expect(field(resources.lastSyncHeartbeat)?.querySelector("time") !== null).toBe(scenario.peers.length > 0);
  }
});
