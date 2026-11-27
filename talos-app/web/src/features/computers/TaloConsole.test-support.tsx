import { StrictMode } from "react";
import { cleanup, render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import type { AppProtocol } from "@talos/sdk-core";
import { afterEach, beforeEach, vi } from "vitest";

import { i18n } from "@/shared/i18n/setup";
import { resetTalosClientFactory, setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { useAppStore } from "@/shared/store/app-store";
import { topologyNodes } from "@/shared/test/computer-topology.fixture";
import { TaloConsole } from "./TaloConsole";
import { useComputers } from "./hooks";

export const readManagedWorker = vi.fn();
export const startManagedWorker = vi.fn();
const stopManagedWorker = vi.fn();
export const resetManagedWorker = vi.fn();
const deleteManagedWorker = vi.fn();
export const listComputers = vi.fn();
export const listFiles = vi.fn();
export const readAutomation = vi.fn();

export const taloInstance: AppProtocol.ManagedWorkerInfo = {
  instanceId: "talo-instance",
  nodeId: "talo-node",
  conversationSessionId: "sess_talo",
  generation: 2,
  phase: "ready",
  operation: null,
  cpuCores: 4,
  memoryBytes: 8 * 1024 ** 3,
  diskQuota: "enforced",
  idleTimeoutSeconds: 600,
  error: null,
};

export let query: QueryClient;

export function QuickNodesProbe() {
  const { quickNodes, nodes, total, selectedId } = useComputers();
  return (
    <>
      <output aria-label="Quick nodes">{quickNodes.map((node) => node.nodeId).join(",")}</output>
      <output aria-label="Listed nodes">{nodes.map((node) => node.nodeId).join(",")}</output>
      <output aria-label="Listed total">{total}</output>
      <output aria-label="Selected node">{selectedId || "none"}</output>
    </>
  );
}

export function showConsole(instance = taloInstance) {
  return render(
    <StrictMode>
      <QueryClientProvider client={query}>
        <MemoryRouter>
          <div className="relative h-[640px] w-[960px]">
            <TaloConsole initialInstance={instance} />
          </div>
        </MemoryRouter>
      </QueryClientProvider>
    </StrictMode>,
  );
}

export function setupTaloConsoleTests() {
  beforeEach(async () => {
    query = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
    await i18n.changeLanguage("en");
    readManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: taloInstance });
    startManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: taloInstance });
    stopManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: { ...taloInstance, phase: "stopped" } });
    resetManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: { ...taloInstance, phase: "resetting", operation: "reset" } });
    deleteManagedWorker.mockReset().mockResolvedValue({ enabled: true, instance: null });
    listComputers.mockReset().mockResolvedValue({ data: topologyNodes, total: 2, nextCursor: null });
    listFiles.mockReset().mockResolvedValue({ data: [], nextCursor: null });
    readAutomation.mockReset().mockResolvedValue({ goal: null, loops: [] });

    useAppStore.setState({
      selectedComputers: {},
      workspaceWindows: [],
      workspaceSnapshots: {},
      workspaceOwner: null,
      workspaceOpen: false,
      connection: {
        ...useAppStore.getInitialState().connection,
        authStatus: "authenticated",
        authRole: "user",
        accountType: "Max",
        userId: "alice",
      },
    });

    setTalosClientFactory(() => ({
      computers: {
        list: listComputers,
        readManagedWorker,
        startManagedWorker,
        stopManagedWorker,
        resetManagedWorker,
        deleteManagedWorker,
        read:  ({ nodeId }: { nodeId: string }) => Promise.resolve(nodeId === taloInstance.nodeId
          ? { ...topologyNodes[0], nodeId, displayName: "Talo", available: true, managedWorkerPhase: "ready" }
          : topologyNodes.find((node) => node.nodeId === nodeId)),
      },
      files: { list: listFiles },
      llm: { get:  () => Promise.resolve(({
        type: "llm",
        config: { provider: "anthropic", model: "fixture", anthropic: { api_key: "", base_url: null, models: [] } },
        provider_keys: {},
        provider_capabilities: { anthropic: { default_protocol: "messages", protocols: ["messages"] } },
      })) },
      automation: { read: readAutomation },
      browser: {
        operate:  () => Promise.resolve(({
          enabled: false,
          host: "docker",
          selectedTargetId: null,
          targets: [],
          display: { kind: "dockerDesktop", targetId: "managed" },
          control: { generation: "gen-1", revision: 1, epoch: 0, owner: { kind: "idle" } },
        })),
      },
      sync: { status:  () => Promise.resolve(({ localComputerId: null })) },
    }) as unknown as TalosWebClient);
  });

  afterEach(() => {
    cleanup();
    query.clear();
    resetTalosClientFactory();
  });
}
