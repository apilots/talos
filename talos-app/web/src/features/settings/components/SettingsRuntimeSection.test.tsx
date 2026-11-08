// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { WorkerRuntimeSettingsResponse } from "@talos/sdk-browser";
import { afterEach, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsRuntimeSection } from "./SettingsRuntimeSection";

const client = vi.hoisted(() => ({ read: vi.fn(), validate: vi.fn(), update: vi.fn() }));
vi.mock("../api", () => ({ workerRuntimeClient: () => client }));

const settings: WorkerRuntimeSettingsResponse = {
  activeBackend: "dockerEngine",
  configuredBackend: "dockerEngine",
  dockerSocketPath: "/var/run/docker.sock",
  kubernetes: {
    namespace: "talos",
    storageClass: "local-path",
    authenticationSource: "inCluster",
    mountedKubeconfigPath: "/var/run/secrets/talos/kubeconfig",
    context: null,
  },
  restartRequired: false,
};

function renderRuntime() {
  client.read.mockResolvedValue(settings);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SettingsRuntimeSection scope="test-server-admin" />
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

it("validates and saves pasted credentials without populating them from Server settings", async () => {
  client.validate.mockResolvedValue(null);
  client.update.mockResolvedValue({
    ...settings,
    configuredBackend: "kubernetes",
    kubernetes: { ...settings.kubernetes, authenticationSource: "pasted", context: "scoped" },
    restartRequired: true,
  });
  renderRuntime();
  const backend = await screen.findByLabelText("Candidate backend");
  expect(screen.getByLabelText("Docker Unix socket")).toHaveProperty("readOnly", true);
  fireEvent.change(backend, { target: { value: "kubernetes" } });
  const authentication = screen.getByLabelText("Kubernetes authentication");
  expect(authentication).toHaveProperty("value", "inCluster");
  fireEvent.change(authentication, { target: { value: "mounted" } });
  expect(screen.getByLabelText("Mounted kubeconfig path")).toHaveProperty("readOnly", true);
  fireEvent.change(authentication, { target: { value: "pasted" } });
  fireEvent.change(screen.getByLabelText("Kubeconfig context"), { target: { value: "scoped" } });
  const credential = screen.getByLabelText("Kubeconfig text");
  expect(credential).toHaveProperty("value", "");
  fireEvent.change(credential, { target: { value: "fixture credential text" } });

  fireEvent.click(screen.getByRole("button", { name: "Validate" }));
  await screen.findByText("Runtime validation passed.");
  expect(client.validate).toHaveBeenCalledWith({
    kind: "kubernetes",
    namespace: "talos",
    storageClass: "local-path",
    authentication: { kind: "pasted", context: "scoped", kubeconfig: "fixture credential text" },
  });
  expect(client.update).not.toHaveBeenCalled();
  expect(credential).toHaveProperty("value", "fixture credential text");

  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await screen.findByText("Restart the Server to apply the saved runtime settings.");
  expect(client.update).toHaveBeenCalledWith(client.validate.mock.calls[0][0]);
  expect(credential).toHaveProperty("value", "");

  fireEvent.click(screen.getByRole("button", { name: "Validate" }));
  await waitFor(() => expect(client.validate).toHaveBeenLastCalledWith({
    kind: "kubernetes",
    namespace: "talos",
    storageClass: "local-path",
    authentication: { kind: "pasted", context: "scoped", kubeconfig: null },
  }));
});
