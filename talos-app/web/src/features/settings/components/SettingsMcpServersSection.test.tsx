// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Plugin } from "@talos/sdk-browser";
import type { PropsWithChildren } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SettingsMcpServersSection } from "./SettingsMcpServersSection";

const apiMocks = vi.hoisted(() => ({
  disablePlugin: vi.fn(),
  enablePlugin: vi.fn(),
  listManagedMcpPlugins: vi.fn(),
  uninstallPlugin: vi.fn(),
  listConnections: vi.fn().mockResolvedValue({ data: [], nextCursor: null }),
  readAuthentication: vi.fn().mockResolvedValue({ authentication: { type: "none" }, state: { type: "unconfigured" } }),
}));

vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ mcp: { list: apiMocks.listConnections, readAuthentication: apiMocks.readAuthentication } }) }));

vi.mock("@/features/plugins/api", () => ({
  disablePlugin: apiMocks.disablePlugin,
  enablePlugin: apiMocks.enablePlugin,
  listManagedMcpPlugins: apiMocks.listManagedMcpPlugins,
  uninstallPlugin: apiMocks.uninstallPlugin,
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function mcpPlugin(enabled: boolean): Plugin {
  return {
    id: "plugin-playwright",
    kind: "mcp_server",
    metadata: {
      name: "playwright",
      display_name: "Playwright",
      description: "Browser automation.",
      short_description: null,
      icon: null,
      license: null,
    },
    documents: [],
    source_id: "source-browser",
    path_in_source: "plugins/playwright",
    source_commit: "abc123",
    status: {
      status: "installed",
      install_id: "install-playwright",
      installed_path: "/tmp/playwright",
      installed_commit: "abc123",
      enabled,
    },
    definition: {
      kind: "mcp_server",
      plugin: {
        manifest_path: "talos-mcp.json",
        server: {
          name: "playwright",
          display_name: "Playwright MCP",
          description: "Playwright MCP server.",
          transport: {
            type: "stdio",
            command: "npx",
            args: ["@playwright/mcp@latest"],
            env: {},
            cwd: null,
          },
        },
      },
    },
    warnings: [],
  };
}

describe("SettingsMcpServersSection", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows only plugin enablement for the built-in Browser", async () => {
    const plugin = mcpPlugin(true); plugin.id = "builtin_browser";
    if (plugin.definition.kind === "mcp_server") plugin.definition.plugin.server.display_name = "Browser";
    apiMocks.listManagedMcpPlugins.mockResolvedValue({ plugins: [plugin], warnings: [] });
    render(<SettingsMcpServersSection />, { wrapper: createWrapper() });
    await screen.findByText("Browser");
    expect(screen.queryByRole("button", { name: "Uninstall" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open MCP plugin" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Disable" })).toBeEnabled();
    expect(screen.queryByRole("textbox", { name: "Command to launch" })).not.toBeInTheDocument();
  });

  it("renders installed MCP plugins from the plugin API", async () => {
    apiMocks.listManagedMcpPlugins.mockResolvedValue({
      plugins: [mcpPlugin(true)],
      warnings: [],
    });

    render(<SettingsMcpServersSection />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText("Playwright MCP")).toBeInTheDocument();
    });
    expect(screen.getByText("stdio")).toBeInTheDocument();
    expect(screen.getByText("source-browser")).toBeInTheDocument();
    expect(screen.getByText("1 server")).toBeInTheDocument();
  });

  it("opens a custom MCP form with required input validation", async () => {
    apiMocks.listManagedMcpPlugins.mockResolvedValue({
      plugins: [],
      warnings: [],
    });

    render(<SettingsMcpServersSection />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText("No MCP server plugins are installed.")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));

    expect(screen.getByText("Connect to a custom MCP")).toBeInTheDocument();
    expect(screen.getByText("STDIO")).toBeInTheDocument();
    expect(screen.getByText("Streamable HTTP")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    expect(screen.getByRole("textbox", { name: "Name" })).toBeInvalid();
    expect(screen.getByRole("textbox", { name: "Command to launch" })).toBeInvalid();
  });

  it("keeps a marketplace MCP definition read-only while exposing lifecycle actions", async () => {
    apiMocks.listManagedMcpPlugins.mockResolvedValue({
      plugins: [mcpPlugin(true)],
      warnings: [],
    });

    render(<SettingsMcpServersSection />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText("Playwright MCP")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Open MCP plugin" }));

    expect(screen.getByText("Update Playwright MCP")).toBeInTheDocument();
    expect(screen.getByDisplayValue("npx")).toBeInTheDocument();
    expect(screen.getByDisplayValue("@playwright/mcp@latest")).toBeInTheDocument();
    expect(screen.getByDisplayValue("npx")).toHaveAttribute("readonly");
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Disable" })).toBeEnabled();
  });

  it("enables and disables installed MCP plugins through plugin actions", async () => {
    apiMocks.listManagedMcpPlugins.mockResolvedValue({
      plugins: [mcpPlugin(true)],
      warnings: [],
    });
    apiMocks.disablePlugin.mockResolvedValue({});

    render(<SettingsMcpServersSection />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText("Playwright MCP")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Disable" }));

    await waitFor(() => {
      expect(apiMocks.disablePlugin).toHaveBeenCalledWith(expect.objectContaining({ id: "plugin-playwright" }));
    });
  });
});
