// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { WorkerRuntimeSettingsResponse } from "@talos/sdk-browser";
import { afterEach, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsRuntimeSection } from "./SettingsRuntimeSection";

const client = vi.hoisted(() => ({ read: vi.fn(), validate: vi.fn(), update: vi.fn() }));
vi.mock("../api", () => ({ workerRuntimeClient: () => client }));

const worker = { image: "ghcr.io/apilots/worker@sha256:fixture", controlPlaneEndpoint: "https://docker.example.test", trustedCaFile: null };
const kubeWorker = { ...worker, controlPlaneEndpoint: "https://talos-server.talos.svc:9443", trustedCaFile: "/run/talos-tls/ca.crt" };
const resources = { cpuCores: 4, memoryBytes: 8 * 1024 ** 3, shmBytes: 1024 ** 3 };

const settings: WorkerRuntimeSettingsResponse = {
  activeBackend: "dockerEngine",
  configuredBackend: "dockerEngine",
  dockerSocketPath: "/var/run/docker.sock",
  profile: { worker, ...resources },
  timing: { idleTimeoutSeconds: 900, requestWaitSeconds: 45, startTimeoutSeconds: 120, wakeCooldownSeconds: 60 },
  kubernetes: {
    seccompProfile: "talos/chromium-seccomp.json",
    homeBytes: 100_000_000_000,
    homeQuotaEnforced: true,
    namespace: "talos",
    storageClass: "local-path",
    authenticationSource: "inCluster",
    mountedKubeconfigPath: "/var/run/secrets/talos/kubeconfig",
    context: null,
  },
  restartRequired: false,
};

function renderRuntime(backend: "dockerEngine" | "kubernetes") {
  client.read.mockResolvedValue({
    ...settings,
    activeBackend: backend,
    configuredBackend: backend,
    profile: backend === "dockerEngine" ? settings.profile : { worker: kubeWorker, ...resources, cpuCores: 6 },
  });
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
    profile: { worker: kubeWorker, ...resources },
    kubernetes: { ...settings.kubernetes, authenticationSource: "pasted", context: "scoped" },
    restartRequired: true,
  });
  renderRuntime("kubernetes");
  const authentication = await screen.findByLabelText("Kubernetes authentication");
  expect(authentication).toHaveProperty("value", "inCluster");
  expect(screen.getByLabelText("Talos Server address for Worker registration")).toHaveProperty("value", kubeWorker.controlPlaneEndpoint);
  expect(screen.getByText("100 GB per new volume")).toBeTruthy();
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
    edits: { worker: null, idleTimeoutSeconds: null },
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
    edits: { worker: null, idleTimeoutSeconds: null },
  }));
});

it("edits only the deployed Docker profile and shared idle policy", async () => {
  client.validate.mockResolvedValue(null);
  renderRuntime("dockerEngine");
  const address = await screen.findByLabelText("Talos Server address for Worker registration");
  expect(address).toHaveProperty("value", worker.controlPlaneEndpoint);
  expect(screen.getByLabelText("Docker Unix socket")).toHaveProperty("readOnly", true);
  expect(screen.getByText("Docker volume without a quota")).toBeTruthy();
  fireEvent.change(address, { target: { value: "https://docker-edited.example.test" } });
  fireEvent.change(screen.getByLabelText("Idle sleep timeout (seconds)"), { target: { value: "600" } });
  fireEvent.click(screen.getByRole("button", { name: "Validate" }));
  await screen.findByText("Runtime validation passed.");
  expect(client.validate).toHaveBeenCalledWith({
    kind: "dockerEngine",
    edits: {
      worker: { ...worker, controlPlaneEndpoint: "https://docker-edited.example.test" },
      idleTimeoutSeconds: 600,
    },
  });
});
