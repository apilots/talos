// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from "vitest";
import { createJSONStorage } from "zustand/middleware";

import { useAppStore } from "./app-store";

describe("app store persistence", () => {
  let storage: Map<string, string>;

  beforeEach(() => {
    storage = new Map<string, string>();
    useAppStore.persist.setOptions({
      storage: createJSONStorage(() => ({
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => {
          storage.set(key, value);
        },
        removeItem: (key: string) => {
          storage.delete(key);
        },
      })),
    });
    useAppStore.setState((state) => ({
      ...state,
      streams: {},
      transientStatuses: {},
    }));
  });

  it("does not persist transient chat runtime state", () => {
    useAppStore.getState().setStreamMeta("sess-1", {
      isStreaming: true,
      connectionState: "detached",
      activeMessageId: "msg-1",
    });
    useAppStore.getState().setSessionStatus("sess-1", "running");

    const raw = storage.get("talos-web-ui");
    expect(raw).toBeTruthy();
    const persisted = JSON.parse(raw ?? "{}") as { state?: Record<string, unknown> };

    expect(persisted.state).not.toHaveProperty("streams");
    expect(persisted.state).not.toHaveProperty("transientStatuses");
  });

  it("drops previously persisted transient chat runtime state during rehydration", async () => {
    storage.set(
      "talos-web-ui",
      JSON.stringify({
        state: {
          streams: {
            "sess-1": {
              isStreaming: true,
              connectionState: "reconnecting",
              blocks: [],
            },
          },
          transientStatuses: {
            "sess-1": "running",
          },
        },
        version: 0,
      }),
    );

    await useAppStore.persist.rehydrate();

    expect(useAppStore.getState().streams).toEqual({});
    expect(useAppStore.getState().transientStatuses).toEqual({});
  });
});
