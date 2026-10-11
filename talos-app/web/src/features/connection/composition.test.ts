// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";
import { AppProtocol } from "@talos/sdk-core";
import { getTalosClient, resetTalosClientFactory } from "@/shared/api/client";
import { getCurrentUserWithToken, getGatewayHealth } from "@/features/auth/api";
import { useAppStore } from "@/shared/store/app-store";

const desktopWindow = window as Window & { talos?: DesktopRendererBridge };
const success = (data: unknown) => ({ status: "success", data });
function installHost(mode: "cloud" | "local") {
  const invoke = vi.fn(async (method: string) => {
    if (method === "desktop/connection/read") return success({ connectionMode: mode, controlPlaneEndpoint: "https://agent.example" });
    if (method === "model/list" && mode === "local") return success({ models: [], defaultModelSelection: null });
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

  it("uses the real cloud actor and product endpoint while the local host only supplies settings", async () => {
    const invoke = installHost("cloud");
    const user = { user_id: "alice", username: "Alice", role: "user", type: "Free" };
    const fetcher = vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(JSON.stringify({ auth_enabled: true, mode: "cloud" })))
      .mockResolvedValueOnce(new Response(JSON.stringify(success({ user }))))
      .mockResolvedValueOnce(new Response(JSON.stringify(success({ models: [], defaultModelSelection: null }))));
    const health = await getGatewayHealth();
    useAppStore.getState().setConnection({ desktopConnectionMode: health.desktopConnection!.connectionMode,
      controlPlaneEndpoint: health.desktopConnection!.controlPlaneEndpoint, authToken: "actor", authStatus: "authenticated", userId: "alice" });
    await expect(getCurrentUserWithToken("actor")).resolves.toEqual(user);
    await getTalosClient().models.list({ nodeId: "computer-a" });
    expect(fetcher.mock.calls.map(([url]) => url)).toEqual([
      "https://agent.example/health", "https://agent.example/v1/user", "https://agent.example/v1/models?nodeId=computer-a",
    ]);
    expect(invoke.mock.calls.map(([method]) => method)).toEqual(["desktop/connection/read", "desktop/connection/read"]);
    expect(fetcher.mock.calls[1][1]?.headers).toMatchObject({ Authorization: "Bearer actor" });
  });

  it("keeps pure local execution on stdio and rejects an uninitialized mode instead of falling back", async () => {
    const invoke = installHost("local");
    expect(() => getTalosClient()).toThrow(expect.objectContaining({ code: AppProtocol.ErrorCode.ServiceUnavailable }));
    const fetcher = vi.spyOn(globalThis, "fetch");
    const health = await getGatewayHealth();
    useAppStore.getState().setConnection({ desktopConnectionMode: health.desktopConnection!.connectionMode, authStatus: "open" });
    await getTalosClient().models.list({ nodeId: "embedded" });
    expect(invoke).toHaveBeenCalledWith("model/list", { nodeId: "embedded" }, expect.anything());
    expect(fetcher).not.toHaveBeenCalled();
  });
});
