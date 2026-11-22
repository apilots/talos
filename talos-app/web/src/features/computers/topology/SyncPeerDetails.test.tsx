// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyMetrics, topologyNodes } from "@/shared/test/computer-topology.fixture";
import { ComputerTopologyList } from "./ComputerTopologyList";
import { SyncPeerDetails } from "./SyncPeerDetails";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(topologyMetrics.observedAt * 1000); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

it.each(["en", "zh-CN"])("expires document activity in list and details while retaining historical heartbeat (%s)", async (locale) => {
  const i18n = createInstance();
  const resources = computerResources[locale as keyof typeof computerResources];
  await i18n.init({ lng: locale, resources: { [locale]: { computers: resources } } });
  const peer = { ...topologyMetrics.peers[0], lastActivityAt: topologyMetrics.observedAt - 5 };
  let metrics: AppProtocol.SyncMetricsResponse | undefined = { ...topologyMetrics, peers: [peer] };
  const show = () => <I18nextProvider i18n={i18n}>
    <ComputerTopologyList nodes={topologyNodes.slice(0, 1)} metrics={metrics} onInspect={vi.fn()} />
    <SyncPeerDetails peers={[peer]} metrics={metrics} />
  </I18nextProvider>;
  const { container, rerender } = render(show());
  const activity = () => screen.getByText(resources.lastActivity, { selector: "dt" }).nextElementSibling;
  const listActivity = () => container.querySelector("tbody tr td:last-child");
  const timestamp = new Date(peer.lastActivityAt * 1000).toLocaleTimeString(locale);
  vi.setSystemTime((topologyMetrics.observedAt + 15) * 1000);
  rerender(show());
  expect(activity()).toHaveTextContent(timestamp);
  expect(listActivity()).toHaveTextContent(timestamp);
  vi.setSystemTime((topologyMetrics.observedAt + 16) * 1000);
  rerender(show());
  expect(activity()).toHaveTextContent("N/A");
  expect(listActivity()).toHaveTextContent("—");
  metrics = undefined;
  rerender(show());
  expect(activity()).toHaveTextContent("N/A");
  expect(screen.getByText(resources.lastSyncHeartbeat).nextElementSibling?.querySelector("time"))
    .toHaveAttribute("datetime", new Date(peer.lastHeartbeatAt! * 1000).toISOString());
  expect(container.querySelectorAll(".cmp-peer dt")).toHaveLength(9);
});

