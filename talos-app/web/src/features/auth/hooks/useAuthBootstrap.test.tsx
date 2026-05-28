// @vitest-environment jsdom

import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createJSONStorage } from "zustand/middleware";

import { useAppStore } from "@/shared/store/app-store";
import { useAuthBootstrap } from "./useAuthBootstrap";

const getGatewayHealthMock = vi.fn();
const getCurrentUserMock = vi.fn();
const getCurrentUserWithTokenMock = vi.fn();
const readDesktopCloudSessionMock = vi.fn();
const storeDesktopCloudSessionMock = vi.fn();
const clearDesktopCloudSessionMock = vi.fn();

vi.mock("../api", () => ({
  clearDesktopCloudSession: () => clearDesktopCloudSessionMock(),
  getCurrentUser: () => getCurrentUserMock(),
  getCurrentUserWithToken: (token: string) => getCurrentUserWithTokenMock(token),
  getGatewayHealth: () => getGatewayHealthMock(),
  readDesktopCloudSession: () => readDesktopCloudSessionMock(),
  storeDesktopCloudSession: (token: string, cloudUrl: string, user: unknown) =>
    storeDesktopCloudSessionMock(token, cloudUrl, user),
}));

describe("useAuthBootstrap", () => {
  beforeEach(() => {
    getGatewayHealthMock.mockReset();
    getCurrentUserMock.mockReset();
    getCurrentUserWithTokenMock.mockReset();
    readDesktopCloudSessionMock.mockReset();
    storeDesktopCloudSessionMock.mockReset();
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
        runtimeMode: "cloud",
      },
    }));
  });

  it("touches desktop cloud session cache after restored token passes user verification", async () => {
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
      expect(storeDesktopCloudSessionMock).toHaveBeenCalledWith(
        "cached-token",
        "http://127.0.0.1:8787",
        user,
      );
    });
    expect(getCurrentUserWithTokenMock).toHaveBeenCalledWith("cached-token");
    expect(getCurrentUserMock).not.toHaveBeenCalled();
    expect(useAppStore.getState().connection.authStatus).toBe("authenticated");
    expect(clearDesktopCloudSessionMock).not.toHaveBeenCalled();
  });
});
