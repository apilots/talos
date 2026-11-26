// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { TalosRuntimeState } from "@talos/sdk-app-server/desktop";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, expect, it, vi } from "vitest";
import { computerResources } from "@/shared/i18n/computerResources";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import { ComputerItem } from "./ComputerItem";

afterEach(cleanup);

async function show(locale: "en" | "zh-CN", props: Partial<React.ComponentProps<typeof ComputerItem>> = {}) {
  const i18n = createInstance();
  await i18n.init({ lng: locale, resources: { [locale]: { computers: computerResources[locale] } }, interpolation: { escapeValue: false } });
  return render(<I18nextProvider i18n={i18n}><ComputerItem node={topologyNodes[0]} onSelect={vi.fn()} {...props} /></I18nextProvider>);
}

const runtimes = ["ready", "starting", "restarting", "stopped", "error", "unknown"] as const;
for (const locale of ["en", "zh-CN"] as const) {
  it.each(runtimes)(`shows Desktop runtime %s instead of directory availability (${locale})`, async (kind) => {
    const labels = computerResources[locale];
    const localRuntime: TalosRuntimeState | undefined = kind === "unknown" ? undefined : { kind, message: null };
    const onSelect = vi.fn();
    const { container } = await show(locale, {
      local: true, localRuntime, onSelect,
      node: { ...topologyNodes[0], available: kind !== "ready", managedWorkerPhase: "sleeping" },
    });
    const button = screen.getByRole("button", { name: `Studio · ${labels.local} · ${labels.localRuntime[kind]}` });
    expect(button).toHaveAttribute("title", button.getAttribute("aria-label"));
    expect(button).toHaveTextContent(labels.localRuntime[kind]);
    expect(container.querySelector(".rounded-full")).toHaveClass(kind === "ready" ? "bg-success" : "bg-muted");
    fireEvent.click(button);
    expect(onSelect).toHaveBeenCalledOnce();
  });

  it.for([false, true])(`keeps remote access policy separate from ready runtime, including compact mode (${locale}, compact=%s)`, async (compact, { expect }) => {
    const labels = computerResources[locale];
    const { container } = await show(locale, {
      local: true, compact, localRuntime: { kind: "ready", message: null },
      node: { ...topologyNodes[0], available: false, status: "disabled" },
    });
    const label = `Studio · ${labels.local} · ${labels.localRuntime.ready} · ${labels.remoteDisabled}`;
    const button = screen.getByRole("button", { name: label });
    expect(button).toHaveAttribute("title", label);
    expect(container.querySelector(".rounded-full")).toHaveClass("bg-success");
    expect(button.querySelector(".block.text-xs")?.outerHTML ?? null).toBe(compact ? null
      : `<span class="block text-xs text-muted">${labels.localRuntime.ready} · ${labels.remoteDisabled}</span>`);
  });
}

it.each([
  { available: true, status: "enabled", managedWorkerPhase: null, label: "Connected" },
  { available: false, status: "enabled", managedWorkerPhase: null, label: "Offline" },
  { available: false, status: "disabled", managedWorkerPhase: null, label: computerResources.en.remoteDisabled },
  { available: false, status: "enabled", managedWorkerPhase: "sleeping", label: computerResources.en.talo.phase.sleeping },
] as const)("keeps Server directory status authoritative for non-Desktop rows ($label)", async ({ label, ...state }) => {
  await show("en", { node: { ...topologyNodes[0], ...state }, localRuntime: { kind: "restarting", message: null } });
  expect(screen.getByRole("button", { name: `Studio · ${label}` })).toBeVisible();
});
