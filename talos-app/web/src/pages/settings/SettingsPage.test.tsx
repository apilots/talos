// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsPage } from "./SettingsPage";

const useSettingsPageMock = vi.fn();

vi.mock("@/features/settings", async () => {
  const actual = await vi.importActual<typeof import("@/features/settings")>("@/features/settings");
  return {
    ...actual,
    SettingsPersonalizationSection: () => <div>Personalization content</div>,
    SettingsAppearanceSection: () => <div>Appearance content</div>,
    SettingsGeneralSection: () => <div>General content</div>,
    SettingsMcpServersSection: () => <div>MCP servers content</div>,
    SettingsModelsSection: () => <div>Models content</div>,
    SettingsProfileSection: () => <div>Profile content</div>,
    SettingsServerSection: () => <div>Advanced content</div>,
    SettingsWebSearchSection: () => <div>Web Search content</div>,
    useSettingsPage: () => useSettingsPageMock(),
  };
});

function buildSettingsState(
  authStatus: "authenticated" | "open" = "authenticated",
  authRole: "user" | "admin" = "user",
) {
  return {
    connection: {
      authStatus,
      authToken: authStatus === "authenticated" ? "cloud-token" : "",
      authRole,
      accountType: "Pro",
      authUsername: authRole,
      userId: `${authRole}-1`,
      runtimeMode: "local",
    },
    setConnection: vi.fn(),
    uiTheme: "light",
    setUiTheme: vi.fn(),
    locale: "en",
    setLocale: vi.fn(),
    draft: {
      runtimeMode: "local",
    },
    setDraft: vi.fn(),
    desktopNotifications: true,
    setDesktopNotifications: vi.fn(),
    models: { refetch: vi.fn() },
  };
}

function renderSettings(
  initialEntry: string,
  authStatus: "authenticated" | "open" = "authenticated",
  authRole: "user" | "admin" = "user",
) {
  useSettingsPageMock.mockReturnValue(buildSettingsState(authStatus, authRole));
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <Routes>
        <Route element={<SettingsPage />} path="/settings" />
        <Route element={<SettingsPage />} path="/settings/:section" />
      </Routes>
    </MemoryRouter>,
  );
}

describe("SettingsPage", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders the routed section content", () => {
    renderSettings("/settings/models", "authenticated", "admin");

    expect(screen.getByText("Models content")).toBeTruthy();
    expect(screen.getByText("Personalization")).toBeTruthy();
  });

  it("only marks the exact routed settings item as current for regular users", () => {
    renderSettings("/settings/personalization");

    expect(screen.getByRole("link", { name: "General" }).getAttribute("aria-current")).toBeNull();
    expect(screen.getByRole("link", { name: "Personalization" }).getAttribute("aria-current")).toBe("page");
  });

  it("allows admin users to access their personalization settings", () => {
    renderSettings("/settings/personalization", "authenticated", "admin");

    expect(screen.getByText("Personalization content")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Personalization" }).getAttribute("aria-current")).toBe("page");
  });

  it("hides model settings for regular authenticated users", () => {
    renderSettings("/settings");

    expect(screen.getByText("Profile content")).toBeTruthy();
    expect(screen.queryByText("Models")).toBeNull();
  });

  it("does not expose disconnected global permission controls", () => {
    renderSettings("/settings");

    expect(screen.queryByRole("link", { name: "Files & Permissions" })).toBeNull();
  });

  it("renders the mcp servers settings section", () => {
    renderSettings("/settings/mcp-servers");

    expect(screen.getByText("MCP servers content")).toBeTruthy();
    expect(screen.getByRole("link", { name: "MCP servers" }).getAttribute("aria-current")).toBe("page");
  });

  it("renders the server settings section", () => {
    renderSettings("/settings/server");

    expect(screen.getByText("Advanced content")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Server" }).getAttribute("aria-current")).toBe("page");
  });

  it("renders the web search settings section", () => {
    renderSettings("/settings/web-search", "authenticated", "admin");

    expect(screen.getByText("Web Search content")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Web Search" }).getAttribute("aria-current")).toBe("page");
  });

  it("redirects regular users away from config management sections", () => {
    renderSettings("/settings/web-search");

    expect(screen.getByText("Profile content")).toBeTruthy();
    expect(screen.queryByText("Web Search content")).toBeNull();
  });

  it("renders the general settings section", () => {
    renderSettings("/settings/general");

    expect(screen.queryByText("Models")).toBeNull();
    expect(screen.getByText("General content")).toBeTruthy();
  });

  it("redirects the removed account page to the settings profile", () => {
    renderSettings("/settings/account", "authenticated", "admin");

    expect(screen.getByText("Profile content")).toBeTruthy();
    expect(screen.queryByText("Account content")).toBeNull();
  });
});
