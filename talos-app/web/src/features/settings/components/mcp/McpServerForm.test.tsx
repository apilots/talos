// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { McpServerForm } from "./McpServerForm";
import { McpCredentialsForm } from "./McpCredentialsForm";

const api = vi.hoisted(() => ({ install: vi.fn(), import: vi.fn(), readAuthentication: vi.fn(), updateCredentials: vi.fn(), startAuthentication: vi.fn() }));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ plugin: { install: api.install }, mcp: api }) }));
vi.mock("react-i18next", async (original) => ({ ...await original<typeof import("react-i18next")>(), useTranslation: () => ({ t: (key: string) => key }) }));
const navigation = vi.hoisted(() => ({ prepareExternalAuthWindow: vi.fn(), redirectToExternalUrl: vi.fn(), closePreparedExternalWindow: vi.fn() }));
vi.mock("@/features/auth/navigation", () => navigation);
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("adds HTTP credentials without placing a token in the definition, and preserves rejected JSON for correction", async () => {
  const saved = vi.fn().mockResolvedValue(undefined);
  api.install.mockResolvedValue({});
  const view = render(<McpServerForm mode="create" onBack={vi.fn()} onSaved={saved} />);
  fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.name" }), { target: { value: "remote" } });
  fireEvent.click(screen.getByRole("button", { name: "mcpServers.streamableHttp" }));
  fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.url" }), { target: { value: "https://example.com/mcp" } });
  fireEvent.change(screen.getByLabelText("mcpServers.secretValue"), { target: { value: "private-token" } });
  fireEvent.click(screen.getByRole("button", { name: "mcpServers.save" }));
  await waitFor(() => expect(saved).toHaveBeenCalledOnce());
  const request = api.install.mock.calls[0][0];
  expect(request.target.credentials.token).toBe("private-token");
  expect(JSON.stringify(request.target.server)).not.toContain("private-token");
  expect(screen.getByLabelText("mcpServers.secretValue")).toHaveValue("");
  view.unmount();
  api.import.mockRejectedValue(new Error("backend rejects legacy SSE"));
  render(<McpServerForm mode="create" onBack={vi.fn()} onSaved={saved} />);
  fireEvent.click(screen.getByRole("checkbox", { name: "mcpServers.importJson" }));
  const json = '{"mcpServers":{"old":{"type":"sse","url":"https://example.com/sse"}}}';
  fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.importJson" }), { target: { value: json } });
  fireEvent.click(screen.getByRole("button", { name: "mcpServers.save" }));
  await screen.findByRole("alert");
  expect(api.import).toHaveBeenCalledWith(expect.objectContaining({ json, conflictPolicy: "reject" }));
  expect(screen.getByRole("textbox", { name: "mcpServers.importJson" })).toHaveValue(json);
});

it.each([
  { from: "http", hiddenEntries: "", description: "HTTP token" },
  { from: "http", hiddenEntries: "unfinished-header", description: "HTTP token and invalid headers" },
  { from: "stdio", hiddenEntries: "unfinished-env", description: "invalid STDIO environment" },
] as const)("saves the selected transport after switching away from $description", async ({ from, hiddenEntries }) => {
  const saved = vi.fn().mockResolvedValue(undefined);
  api.install.mockResolvedValue({});
  render(<McpServerForm mode="create" onBack={vi.fn()} onSaved={saved} />);
  fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.name" }), { target: { value: "switched" } });
  if (from === "http") {
    fireEvent.click(screen.getByRole("button", { name: "mcpServers.streamableHttp" }));
    fireEvent.change(screen.getByLabelText("mcpServers.secretValue"), { target: { value: "hidden-token" } });
    fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.headers" }), { target: { value: hiddenEntries } });
    fireEvent.click(screen.getByRole("button", { name: "mcpServers.stdio" }));
    fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.command" }), { target: { value: "npx" } });
    fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.environmentVariables" }), { target: { value: "MODE=test" } });
  } else {
    fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.environmentVariables" }), { target: { value: hiddenEntries } });
    fireEvent.click(screen.getByRole("button", { name: "mcpServers.streamableHttp" }));
    fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.url" }), { target: { value: "https://example.com/mcp" } });
    fireEvent.change(screen.getByRole("textbox", { name: "mcpServers.headers" }), { target: { value: "X-Workspace=test" } });
  }
  fireEvent.click(screen.getByRole("button", { name: "mcpServers.save" }));
  await waitFor(() => expect(saved).toHaveBeenCalledOnce());
  expect(api.install).toHaveBeenCalledOnce();
  expect(api.install.mock.calls[0][0].target).toEqual({
    kind: "manual_mcp_server",
    server: {
      name: "switched", display_name: null, description: null,
      transport: from === "http"
        ? { type: "stdio", command: "npx", args: [], env: { MODE: "test" }, cwd: null }
        : { type: "streamable_http", url: "https://example.com/mcp", headers: { "X-Workspace": "test" } },
    },
  });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("replaces an API key, wipes the input after saving, then clears credentials for the same definition", async () => {
  const server = { type: "config" as const, name: "static" };
  api.readAuthentication.mockResolvedValue({ server, authentication: { type: "header", name: "X-API-Key" }, state: { type: "credentialsStored" } });
  api.updateCredentials.mockResolvedValue({});
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(["talos", "mcp-auth", server], { server, authentication: { type: "none" }, state: { type: "unconfigured" } });
  render(<QueryClientProvider client={client}><McpCredentialsForm server={server} stdio={false} /></QueryClientProvider>);
  await waitFor(() => expect(screen.getByRole("combobox")).toHaveValue("header"));
  expect(screen.getByRole("textbox", { name: "mcpServers.headerName" })).toHaveValue("X-API-Key");
  fireEvent.change(screen.getByLabelText("mcpServers.secretValue"), { target: { value: "replacement" } });
  fireEvent.click(screen.getByRole("button", { name: "mcpServers.replaceCredentials" }));
  await waitFor(() => expect(screen.getByLabelText("mcpServers.secretValue")).toHaveValue(""));
  expect(api.updateCredentials).toHaveBeenCalledWith({ server, change: { type: "replace", credentials: { authentication: { type: "header", name: "X-API-Key" }, token: "replacement" } } });
  fireEvent.click(screen.getByRole("button", { name: "mcpServers.clearCredentials" }));
  await waitFor(() => expect(api.updateCredentials).toHaveBeenLastCalledWith({ server, change: { type: "clear" } }));
  client.clear();
});


it("opens a browser grant and refreshes the definition even when authorization completes before polling", async () => {
  const server = { type: "config" as const, name: "oauth" };
  api.readAuthentication.mockResolvedValue({ server, authentication: { type: "oauth", config: { scopes: ["read"] } }, state: { type: "authenticated" } });
  api.startAuthentication.mockImplementation(async () => {
    api.readAuthentication.mockResolvedValue({ server, authentication: { type: "oauth", config: { scopes: ["read"] } }, state: { type: "authenticated" } });
    return { authorizationUrl: "https://issuer.example/authorize?state=private", attemptId: "attempt", expiresAt: 123, server };
  });
  const updated = vi.fn().mockResolvedValue(undefined);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}><McpCredentialsForm server={server} stdio={false} onUpdated={updated} /></QueryClientProvider>);
  await screen.findByRole("status");
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "oauth" } });
  fireEvent.change(screen.getByLabelText("mcpServers.oauthScopes"), { target: { value: "read" } });
  fireEvent.click(screen.getByRole("button", { name: "mcpServers.oauthAuthorize" }));
  await waitFor(() => expect(navigation.redirectToExternalUrl).toHaveBeenCalledWith("https://issuer.example/authorize?state=private", undefined));
  expect(JSON.stringify(document.body.innerHTML)).not.toContain("state=private");
  await waitFor(() => expect(updated).toHaveBeenCalledOnce());
  client.clear();
});
