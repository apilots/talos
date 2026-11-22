// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import { ComputerDetails } from "./ComputerDetails";

afterEach(cleanup);

it.each(["en", "zh-CN"])("retains metadata and field layout when unavailable or missing (%s)", async (locale) => {
  const i18n = createInstance();
  await i18n.init({ lng: locale, resources: { [locale]: { computers: computerResources[locale as keyof typeof computerResources] } } });
  const systemInfo: AppProtocol.NodeSystemInfo = {
    os: "linux", osVersion: "Ubuntu 24.04", architecture: "x86_64", hostname: "retained-build-worker.example",
    cpuModel: "Fixture CPU", logicalCpuCount: 2, memoryBytes: 4 * 1024 ** 3, boardModel: null,
    graphics: [], networkInterfaces: [{ name: "eth0", macAddress: "00:11:22:33:44:55" }], workerVersion: "0.1.18",
  };
  let node = { ...topologyNodes[0], systemInfo } as AppProtocol.NodeInfo;
  const show = () => <I18nextProvider i18n={i18n}><ComputerDetails node={node} controlPlane="https://control.example" /></I18nextProvider>;
  const { container, rerender } = render(show());
  const labels = () => [...container.querySelectorAll("div > dl > div > dt")].map((row) => row.textContent);
  const originalLabels = labels();
  for (const health of ["healthy", "unknown", "degraded"] as const) {
    node = { ...node, available: false, health };
    rerender(show());
    expect(labels()).toEqual(originalLabels);
    expect(screen.getByText(systemInfo.hostname!)).toBeVisible();
    expect(screen.getByText("4 GiB")).toBeVisible();
    expect(screen.getByText(node.nodeId)).toBeVisible();
  }
  node = { ...node, systemInfo: null };
  rerender(show());
  expect(labels()).toEqual(originalLabels);
  expect(container.querySelectorAll("div > dl > div")).toHaveLength(15);
  expect(screen.getAllByText("N/A").length).toBeGreaterThan(8);
  expect(screen.getByText(node.nodeId)).toBeVisible();
  expect(screen.getByText(/N\/A/, { selector: "summary" })).toBeVisible();
});
