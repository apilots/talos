// @vitest-environment jsdom

import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createJSONStorage } from "zustand/middleware";

import { useAppStore } from "@/shared/store/app-store";
import { useAuthBootstrap } from "./useAuthBootstrap";

const getGatewayHealthMock = vi.fn();
const getCurrentUserMock = vi.fn();
const getCurrentUserWithTokenMock = vi.fn();
const readDesktopCloudSessionMock = vi.fn();
const clearDesktopCloudSessionMock = vi.fn();

vi.mock("../api", () => ({
  clearDesktopCloudSession: () => clearDesktopCloudSessionMock(),
  getCurrentUser: () => getCurrentUserMock(),
  getCurrentUserWithToken: (token: string) => getCurrentUserWithTokenMock(token),
  getGatewayHealth: () => getGatewayHealthMock(),
  readDesktopCloudSession: () => readDesktopCloudSessionMock(),
}));

describe("useAuthBootstrap", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    getGatewayHealthMock.mockReset();
    getCurrentUserMock.mockReset();
    getCurrentUserWithTokenMock.mockReset();
    readDesktopCloudSessionMock.mockReset();
    clearDesktopCloudSessionMock.mockReset();
    const storage = new Map<string, string>();
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
        serverUrl: "http://127.0.0.1:8080",
        authToken: "",
        authStatus: "unknown",
        authRole: "user",
        authUsername: "Anonymous",
        userId: "anonymous",
        runtimeMode: "local",
      },
    }));
  });

  it("restores desktop cloud session cache after token passes user verification", async () => {
    const user = {
      user_id: "user-1",
      username: "ada",
      email: "ada@example.com",
      role: "admin",
      disabled: false,
      created_at: 0,
      updated_at: 0,
    };
    getGatewayHealthMock.mockResolvedValue({ auth_enabled: true });
    readDesktopCloudSessionMock.mockResolvedValue({
      access_token: "cached-token",
      cloud_api_base_url: "http://127.0.0.1:8787",
      user: {
        user_id: "user-1",
        username: "ada",
        role: "admin",
      },
    });
    getCurrentUserWithTokenMock.mockResolvedValue(user);

    renderHook(() => useAuthBootstrap());

    await waitFor(() => {
      expect(useAppStore.getState().connection.authStatus).toBe("authenticated");
    });
    expect(getCurrentUserWithTokenMock).toHaveBeenCalledWith("cached-token");
    expect(getCurrentUserMock).not.toHaveBeenCalled();
    expect(useAppStore.getState().connection.authStatus).toBe("authenticated");
    expect(clearDesktopCloudSessionMock).not.toHaveBeenCalled();
  });

  it("verifies an in-memory browser bearer token", async () => {
    const user = {
      user_id: "user-2",
      username: "linus",
      email: null,
      role: "user",
      disabled: false,
      created_at: 0,
      updated_at: 0,
    };
    useAppStore.getState().setConnection({ authToken: "memory-token" });
    getGatewayHealthMock.mockResolvedValue({ auth_enabled: true });
    readDesktopCloudSessionMock.mockResolvedValue(null);
    getCurrentUserWithTokenMock.mockResolvedValue(user);

    renderHook(() => useAuthBootstrap());

    await waitFor(() => {
      expect(useAppStore.getState().connection.authStatus).toBe("authenticated");
    });
    expect(getCurrentUserWithTokenMock).toHaveBeenCalledWith("memory-token");
    expect(getCurrentUserMock).not.toHaveBeenCalled();
    expect(readDesktopCloudSessionMock).not.toHaveBeenCalled();
    expect(clearDesktopCloudSessionMock).not.toHaveBeenCalled();
  });

  it("marks the connection unauthenticated when bearer verification fails", async () => {
    getGatewayHealthMock.mockResolvedValue({ auth_enabled: true });
    readDesktopCloudSessionMock.mockResolvedValue(null);
    getCurrentUserMock.mockRejectedValue(new Error("401 Unauthorized"));

    renderHook(() => useAuthBootstrap());

    await waitFor(() => {
      expect(useAppStore.getState().connection.authStatus).toBe("unauthenticated");
    });
    expect(getCurrentUserMock).toHaveBeenCalledTimes(1);
    expect(getCurrentUserWithTokenMock).not.toHaveBeenCalled();
    expect(clearDesktopCloudSessionMock).toHaveBeenCalledTimes(1);
  });

  it("does not restore a session while the UI is in logged out state", async () => {
    useAppStore.getState().setConnection({ authStatus: "logged_out" });

    renderHook(() => useAuthBootstrap());

    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(getGatewayHealthMock).not.toHaveBeenCalled();
    expect(readDesktopCloudSessionMock).not.toHaveBeenCalled();
    expect(getCurrentUserWithTokenMock).not.toHaveBeenCalled();
    expect(useAppStore.getState().connection.authStatus).toBe("logged_out");
  });
});
