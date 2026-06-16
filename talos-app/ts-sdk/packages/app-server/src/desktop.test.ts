import { describe, expect, it } from "vitest";

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
    ): Promise<T> {
      calls.push({ method, params });
      return { data: [{ id: "model-a", name: "Model A" }] } as T;
    };
    const bridge: DesktopRendererBridge = {
      invoke,
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = new ElectronAppServerClient(bridge);

    const models = await client.listModels();

    expect(models).toEqual([{ id: "model-a", name: "Model A" }]);
    expect(calls).toEqual([{ method: "model.list", params: {} }]);
  });

  it("subscribes to app-server events through the renderer bridge", () => {
    let subscribed = false;
    const invoke: DesktopTalosInvoke = async function <T>(): Promise<T> {
      return undefined as T;
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
        protocol_version: { major: 0, minor: 3 },
        server_info: {
          name: "talos-app-server",
          version: "0.1.2",
          auth_enabled: false,
          cloud_auth_url: "http://127.0.0.1:8787",
        },
      }),
      invoke: async function <T>(): Promise<T> {
        return undefined as T;
      },
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };

    await expect(getDesktopServerInfo(bridge)).resolves.toMatchObject({
      name: "talos-app-server",
      version: "0.1.2",
      auth_enabled: false,
      cloud_auth_url: "http://127.0.0.1:8787",
    });
  });
});
