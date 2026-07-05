// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { getCloudAuthUrl } from "@/features/auth";
import type { SettingsPageState } from "../hooks/useSettingsPage";
import { SettingsAccountSection } from "./SettingsAccountSection";

vi.mock("@/features/auth", () => ({
  getCloudAuthUrl: vi.fn(),
}));

const getCloudAuthUrlMock = vi.mocked(getCloudAuthUrl);

function buildConnection(
  authRole: "user" | "admin" = "user",
  authStatus: "authenticated" | "open" = "authenticated",
): SettingsPageState["connection"] {
  return {
    authRole,
    authStatus,
    authToken: authStatus === "authenticated" ? "cloud-token" : "",
    accountType: "Pro",
    authUsername: authRole,
    runtimeMode: "local",
    userId: `${authRole}-1`,
  };
}

describe("SettingsAccountSection", () => {
  beforeEach(() => {
    getCloudAuthUrlMock.mockResolvedValue("http://127.0.0.1:8787");
    vi.stubGlobal("open", vi.fn());
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("opens Cloud account management without passing a token in the URL", async () => {
    render(<SettingsAccountSection connection={buildConnection()} />);

    await waitFor(() => expect(screen.getByText("127.0.0.1:8787")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /Manage account/i }));

    expect(window.open).toHaveBeenCalledWith(
      "http://127.0.0.1:8787/account",
      "_blank",
      "noopener,noreferrer",
    );
    expect(screen.queryByText("Manage users")).toBeNull();
  });

  it("shows Cloud user management only for admin users", async () => {
    render(<SettingsAccountSection connection={buildConnection("admin")} />);

    await waitFor(() => expect(screen.getByText("127.0.0.1:8787")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /Manage users/i }));

    expect(window.open).toHaveBeenCalledWith(
      "http://127.0.0.1:8787/admin/users",
      "_blank",
      "noopener,noreferrer",
    );
  });

  it("does not open Cloud management in open mode", async () => {
    render(<SettingsAccountSection connection={buildConnection("user", "open")} />);

    await waitFor(() => expect(screen.getByText("127.0.0.1:8787")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /Manage account/i }));

    expect(window.open).not.toHaveBeenCalled();
    expect(screen.getByText("Login with Talos Cloud before opening account management.")).toBeTruthy();
  });
});
