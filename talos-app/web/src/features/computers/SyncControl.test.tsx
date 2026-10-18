// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SyncControl } from "./SyncControl";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { topologyMetrics } from "@/shared/test/computer-topology.fixture";
import { useAppStore } from "@/shared/store/app-store";
import { setTalosClientFactory, resetTalosClientFactory, type TalosWebClient } from "@/shared/api/client";

vi.mock("react-i18next", async (importOriginal) => ({ ...await importOriginal<typeof import("react-i18next")>(), useTranslation: () => ({ t: (key: string) => key }) }));
const off = { connection: "disabled", configuredEnabled: false, pendingChanges: 2,
  lastAcknowledgedAt: null, lastError: null, localComputerId: "mine", canClaimLocalHistory: true };
const sync = { status: vi.fn(), enable: vi.fn(), disable: vi.fn() };
function show(metrics?: typeof topologyMetrics, metricsError?: unknown) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}><MemoryRouter><SyncControl metrics={metrics} metricsError={metricsError} /></MemoryRouter></QueryClientProvider>);
}
beforeEach(() => {
  vi.resetAllMocks();
  useAppStore.setState({ connection: { ...useAppStore.getInitialState().connection, authStatus: "authenticated", userId: "alice" } });
  setTalosClientFactory(() => ({ sync } as unknown as TalosWebClient));
});
afterEach(() => { cleanup(); resetTalosClientFactory(); });
it("requires explicit consent to claim local history and preserves pending changes while sharing is off", async () => {
  sync.status.mockResolvedValue(off);
  sync.enable.mockResolvedValue({ ...off, connection: "connected", configuredEnabled: true });
  show();
  await screen.findByRole("checkbox");
  expect(screen.getByRole("status").textContent).toContain("syncPending");
  expect(screen.getByRole("checkbox")).not.toBeChecked();
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("switch"));
  await waitFor(() => expect(sync.enable).toHaveBeenCalledWith({ claimLocalHistory: true }));
});
it("offers a sharing-off switch when H is disconnected without adding an execution lock", async () => {
  sync.status.mockResolvedValue({ ...off, connection: "disconnected", configuredEnabled: true });
  sync.disable.mockResolvedValue(off);
  show();
  const toggle = await screen.findByRole("switch");
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).toHaveAttribute("aria-checked", "true");
  fireEvent.click(toggle);
  await waitFor(() => expect(sync.disable).toHaveBeenCalledOnce());
});

it("shows a conflict instead of reporting connected data as up to date", async () => {
  sync.status.mockResolvedValue({ ...off, connection: "connected", configuredEnabled: true,
    lastError: { code: "conflict", message: "Conflict", localization: null } });
  show();
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("syncConflict"));
  expect(screen.getByRole("status")).toHaveTextContent("syncPending");
  expect(screen.getByRole("alert")).toBeVisible();
});

it("uses document observations for the same active label as topology", async () => {
  sync.status.mockResolvedValue({ ...off, connection: "connected", peerId: "peer-a", configuredEnabled: true, pendingChanges: 0 });
  show(topologyMetrics);
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("syncActive"));
});

it.each(["disconnected", "connecting"])("shows one localized transport notice while %s despite duplicate status/metrics failures", async (connection) => {
  const error = { code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Synchronization service is unavailable", localization: null };
  sync.status.mockResolvedValue({ ...off, connection, configuredEnabled: true, lastError: error });
  show(undefined, new TalosApiError(error));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("syncUnavailable"));
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(screen.queryByText(error.message)).toBeNull();
});

it("preserves a specific conflict when telemetry fails too", async () => {
  sync.status.mockResolvedValue({ ...off, connection: "connected", configuredEnabled: true,
    lastError: { code: "conflict", message: "The stored Worker binding differs.", localization: null } });
  show(undefined, new TalosApiError({ code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Metrics unavailable", localization: null }));
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("syncConflict"));
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(screen.getByRole("alert")).toHaveTextContent("The stored Worker binding differs.");
});
