// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";
import { getTalosClient, resetTalosClientFactory } from "@/shared/api/client";
import { getCurrentUserWithToken, getGatewayHealth } from "@/features/auth/api";
import { useAppStore } from "@/shared/store/app-store";

const desktopWindow = window as Window & { talos?: DesktopRendererBridge };
const success = (data: unknown) => ({ status: "success", data });
function installHost() {
  const invoke = vi.fn(async (method: string) => {
    if (method === "desktop/workerIdentity") return success({ nodeId: "computer-a", controlPlaneEndpoint: "https://agent.example" });
    if (method === "model/list") return success({ models: [], defaultModelSelection: null });
    if (method === "user/current") return success({ user: { user_id: "alice", username: "Alice", role: "user", type: "Free" } });
    throw new Error("Unexpected host business request");
  });
  desktopWindow.talos = {
    invoke,
    connect: async () => success({ server_info: { name: "Talos", auth_enabled: false, cloud_auth_url: null } }),
    onEvent: () => () => undefined,
    onSubscriptionTerminated: () => () => undefined,
  } as unknown as DesktopRendererBridge;
  return invoke;
}

describe("Desktop product composition", () => {
  beforeEach(() => {
    resetTalosClientFactory();
    useAppStore.setState({ connection: { ...useAppStore.getInitialState().connection } });
  });
  afterEach(() => { delete desktopWindow.talos; vi.restoreAllMocks(); });

  it("keeps authenticated Desktop business calls on its own management host with the verified actor", async () => {
    const invoke = installHost();
    const fetcher = vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("Public G offline"));
    const health = await getGatewayHealth();
    useAppStore.getState().setConnection({ controlPlaneEndpoint: health.desktopWorker!.controlPlaneEndpoint!, authToken: "actor", authStatus: "authenticated", userId: "alice" });
    await expect(getCurrentUserWithToken("actor")).resolves.toMatchObject({ user_id: "alice" });
    await getTalosClient().models.list({ nodeId: "computer-a" });
    expect(invoke).toHaveBeenCalledWith("user/current", {}, { actorCredentials: { credential: { type: "existing_session_token", token: "actor" } } });
    expect(invoke).toHaveBeenCalledWith("model/list", { nodeId: "computer-a" }, expect.objectContaining({ actorCredentials: expect.anything() }));
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("keeps anonymous execution on its local stdio Worker", async () => {
    const invoke = installHost();
    const fetcher = vi.spyOn(globalThis, "fetch");
    await getGatewayHealth();
    useAppStore.getState().setConnection({ authStatus: "open" });
    await getTalosClient().models.list({ nodeId: "embedded" });
    expect(invoke).toHaveBeenCalledWith("model/list", { nodeId: "embedded" }, expect.anything());
    expect(fetcher).not.toHaveBeenCalled();
  });
});
