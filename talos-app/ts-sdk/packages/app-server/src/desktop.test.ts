import { describe, expect, it } from "vitest";

import {
  ElectronAppServerClient,
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
    };
    const client = new ElectronAppServerClient(bridge);

    const unsubscribe = client.onEvent(() => undefined);
    expect(subscribed).toBe(true);
    unsubscribe();
    expect(subscribed).toBe(false);
  });
});
