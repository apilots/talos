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
      connection: {
        authToken: "",
        authStatus: "unknown",
        authRole: "user",
        accountType: "Free",
        authUsername: "Anonymous",
        userId: "anonymous",
      },
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

  it("does not persist browser auth token or user snapshot", () => {
    useAppStore.getState().setConnection({
      authToken: "cloud-token",
      authStatus: "authenticated",
      authRole: "admin",
      accountType: "Max",
      authUsername: "ada",
      userId: "user-1",
    });

    const raw = storage.get("talos-web-ui");
    expect(raw).toBeTruthy();
    const persisted = JSON.parse(raw ?? "{}") as {
      state?: { connection?: Record<string, unknown> };
    };

    expect(persisted.state?.connection).toBeUndefined();
  });

  it("drops previously persisted browser auth token during rehydration", async () => {
    storage.set(
      "talos-web-ui",
      JSON.stringify({
        state: {
          connection: {
            serverUrl: "http://127.0.0.1:8080",
            authToken: "old-token",
            authStatus: "authenticated",
            authRole: "admin",
            accountType: "Max",
            authUsername: "ada",
            userId: "user-1",
          },
        },
        version: 0,
      }),
    );

    await useAppStore.persist.rehydrate();

    expect(useAppStore.getState().connection).toMatchObject({
      authToken: "",
      authStatus: "unknown",
      authRole: "user",
      accountType: "Free",
      authUsername: "Anonymous",
      userId: "anonymous",
    });
  });

  it("ignores previously persisted server URL during rehydration", async () => {
    storage.set(
      "talos-web-ui",
      JSON.stringify({
        state: {
          connection: {
            serverUrl: "http://localhost:8080",
          },
        },
        version: 0,
      }),
    );

    await useAppStore.persist.rehydrate();

    expect(useAppStore.getState().connection).not.toHaveProperty("serverUrl");
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
