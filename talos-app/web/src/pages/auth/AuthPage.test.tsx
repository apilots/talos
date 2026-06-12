// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createJSONStorage } from "zustand/middleware";

import "@/shared/i18n/setup";
import {
  getCloudLoginStatus,
  readDesktopCloudSession,
  startCloudLogin,
} from "@/features/auth";
import { redirectToExternalUrl } from "@/features/auth/navigation";
import { useAppStore } from "@/shared/store/app-store";
import { AuthPage } from "./AuthPage";

vi.mock("@/features/auth", () => ({
  getCloudLoginStatus: vi.fn(),
  readDesktopCloudSession: vi.fn(),
  startCloudLogin: vi.fn(),
}));

vi.mock("@/features/auth/navigation", () => ({
  redirectToExternalUrl: vi.fn(),
}));

const getCloudLoginStatusMock = vi.mocked(getCloudLoginStatus);
const readDesktopCloudSessionMock = vi.mocked(readDesktopCloudSession);
const startCloudLoginMock = vi.mocked(startCloudLogin);
const redirectToExternalUrlMock = vi.mocked(redirectToExternalUrl);

function renderAuthPage(initialEntry: string) {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <Routes>
        <Route element={<AuthPage />} path="/auth/login" />
        <Route element={<AuthPage />} path="/auth/logout" />
        <Route element={<div>Talos Home</div>} path="/" />
        <Route element={<div>Talos Settings</div>} path="/settings" />
      </Routes>
    </MemoryRouter>,
  );
}

describe("AuthPage", () => {
  beforeEach(() => {
    startCloudLoginMock.mockResolvedValue({
      login_id: "dls_1",
      login_url: "https://cloud.talos.love/device?session_id=dls_1&user_code=ABCD-EFGH",
      user_code: "ABCD-EFGH",
      expires_at_ms: Date.now() + 600_000,
      interval_ms: 1,
    });
    getCloudLoginStatusMock.mockResolvedValue({
      status: "authenticated",
      user: {
        user_id: "user-1",
        username: "xiliu",
        email: null,
        role: "user",
        disabled: false,
        created_at: 0,
        updated_at: 0,
      },
    });
    readDesktopCloudSessionMock.mockResolvedValue(null);
    redirectToExternalUrlMock.mockReset();
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
        ...state.connection,
        authToken: "",
        authStatus: "unauthenticated",
        authRole: "user",
        authUsername: "Anonymous",
        userId: "anonymous",
      },
    }));
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("does not start device login when the login page mounts", async () => {
    renderAuthPage("/auth/login?next=/settings");

    expect(screen.getByAltText("Talos")).toBeTruthy();
    expect(screen.getByText("Sign in to Talos")).toBeTruthy();
    expect(startCloudLoginMock).not.toHaveBeenCalled();
    expect(redirectToExternalUrlMock).not.toHaveBeenCalled();
  });

  it("starts device login and opens the Cloud device page after user action", async () => {
    renderAuthPage("/auth/login?next=/settings");

    fireEvent.click(screen.getByRole("button", { name: /continue with talos cloud/i }));

    await waitFor(() => expect(startCloudLoginMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(redirectToExternalUrlMock).toHaveBeenCalledTimes(1));
    const target = new URL(redirectToExternalUrlMock.mock.calls[0]?.[0] as string);

    expect(target.origin).toBe("https://cloud.talos.love");
    expect(target.pathname).toBe("/device");
    expect(target.searchParams.get("session_id")).toBe("dls_1");
  });

  it("marks the connection authenticated after device login is approved", async () => {
    renderAuthPage("/auth/login?next=/settings");

    fireEvent.click(screen.getByRole("button", { name: /continue with talos cloud/i }));

    await waitFor(() => expect(getCloudLoginStatusMock).toHaveBeenCalledWith("dls_1"));
    await waitFor(() => expect(useAppStore.getState().connection.authStatus).toBe("authenticated"));
    expect(useAppStore.getState().connection.authUsername).toBe("xiliu");
    expect(useAppStore.getState().connection.authToken).toBe("");
    expect(redirectToExternalUrlMock).toHaveBeenCalledTimes(1);
  });

  it("shows logout success without starting device login", async () => {
    useAppStore.getState().setConnection({ authStatus: "logged_out" });

    renderAuthPage("/auth/logout");

    expect(screen.getByText("Signed out of Talos")).toBeTruthy();
    expect(startCloudLoginMock).not.toHaveBeenCalled();
    expect(redirectToExternalUrlMock).not.toHaveBeenCalled();
  });
});
