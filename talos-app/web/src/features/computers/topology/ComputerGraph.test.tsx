// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, expect, it, vi } from "vitest";
import { computerResources } from "@/shared/i18n/computerResources";
import { ComputerGraph } from "./ComputerGraph";
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

it("draws the Control Plane sync edge only when its exact peer is observed", async () => {
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
  expect(container.querySelector('[data-service="controlPlane"]')).toBeNull();
  metrics.peers = [
    ...metrics.peers,
    { ...metrics.peers[0], peerId: "peer-control", computerIds: [] },
  ];
  rerender(show());
  expect(
    container.querySelector('[data-flow="sync"][data-service="controlPlane"]'),
  ).not.toBeNull();
});
