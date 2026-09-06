import { describe, expect, it } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";

import {
  ElectronAppServerClient,
  getDesktopServerInfo,
  type DesktopRendererBridge,
  type DesktopTalosInvoke,
  type DesktopTalosMethod,
} from "./desktop";

describe("ElectronAppServerClient", () => {
  it("reuses the shared app-server client surface", async () => {
    const calls: Array<{ method: string; params: unknown }> = [];
    const invoke: DesktopTalosInvoke = async function <T>(
      method: DesktopTalosMethod,
      params: unknown,
    ): Promise<AppProtocol.ApiResponse<T>> {
      calls.push({ method, params });
      return { status: "success", data: { models: [{ id: "model-a", name: "Model A" }] } as T };
    };
    const bridge: DesktopRendererBridge = {
      invoke,
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = new ElectronAppServerClient(bridge);

    const models = await client.listModels();

    expect(models).toEqual({ models: [{ id: "model-a", name: "Model A" }] });
    expect(calls).toEqual([{ method: "model/list", params: {} }]);
  });

  it("subscribes to app-server events through the renderer bridge", () => {
    let subscribed = false;
    const invoke: DesktopTalosInvoke = async function <T>(): Promise<AppProtocol.ApiResponse<T>> {
      return { status: "success", data: undefined as T };
    };
    const bridge: DesktopRendererBridge = {
      invoke,
      onEvent: () => {
        subscribed = true;
        return () => {
          subscribed = false;
        };
      },
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = new ElectronAppServerClient(bridge);

    const unsubscribe = client.onEvent(() => undefined);
    expect(subscribed).toBe(true);
    unsubscribe();
    expect(subscribed).toBe(false);
  });

  it("reads initialize server info through the desktop bridge", async () => {
    const bridge: DesktopRendererBridge = {
      connect: async () => ({
        status: "success", data: {
        protocol_version: AppProtocol.PROTOCOL_VERSION,
        server_info: {
          name: "talos-app-server",
          version: "0.1.5",
          auth_enabled: false,
          cloud_auth_url: "http://127.0.0.1:8787",
        },
      } }),
      invoke: async function <T>(): Promise<AppProtocol.ApiResponse<T>> {
        return { status: "success", data: undefined as T };
      },
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };

    await expect(getDesktopServerInfo(bridge)).resolves.toMatchObject({
      name: "talos-app-server",
          version: "0.1.5",
      auth_enabled: false,
      cloud_auth_url: "http://127.0.0.1:8787",
    });
  });

  it.each([AppProtocol.ErrorCode.InternalError, AppProtocol.ErrorCode.InvalidRequest])(
    "reconstructs %s after structured clone without losing its reason", async (code) => {
      const apiError = { code, message: "The requested configuration cannot be loaded.", localization: null };
      const client = new ElectronAppServerClient({
        invoke: async () => structuredClone({ status: "error" as const, error: apiError }),
        onEvent: () => () => undefined,
        onSubscriptionTerminated: () => () => undefined,
      });
      await expect(client.listModels()).rejects.toEqual(new TalosApiError(apiError));
    },
  );
});
