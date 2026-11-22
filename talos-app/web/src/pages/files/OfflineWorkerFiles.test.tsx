// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import type { AppProtocol } from "@talos/sdk-browser";
import { MemoryRouter } from "react-router-dom";
import { ChatSessionContent } from "../chat/ChatSessionPage";
import "@/shared/i18n/setup";
import { FilesPage } from "./FilesPage";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { getTalosClient } from "@/shared/api/client";
import { productQueries } from "@/shared/api/product-queries";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  productQueries.clear();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(window, "talos");
  useAppStore.setState(useAppStore.getInitialState());
});

async function offlineOrdinaryWorker(verifyNode: boolean) {
  const invoke = vi.fn();
  Object.defineProperty(window, "talos", { configurable: true, value: { invoke } });
  const http = vi.fn();
  vi.stubGlobal("fetch", http);
  useAppStore.getState().setConnection({ authStatus: "authenticated", userId: "alice",
    accountType: "Max", authToken: "alice-token", controlPlaneEndpoint: "https://server.example" });
  const nodeId = "registered-worker";
  const sessionId = "saved-chat";
  const projectId = "project";
  const file = { fileId: "notes", originalFilename: "notes.txt", mimeType: "text/plain", tags: [],
    sizeBytes: 5, metadata: {}, createdAt: 1, updatedAt: 1, scope: { type: "project", projectId } as const };
  if (verifyNode) {
    http.mockResolvedValueOnce(Response.json({ status: "success", data: { nodeId, managedWorkerPhase: null } }));
    await getTalosClient({ nodeId });
    expect(http).toHaveBeenCalledWith(expect.stringContaining(nodeId), expect.objectContaining({
      headers: expect.objectContaining({ Authorization: "Bearer alice-token" }),
    }));
    productQueries.clear();
  }
  http.mockReset().mockRejectedValue(new TypeError("Server offline"));
  invoke.mockImplementation(async (method: string, params: AppProtocol.FileUploadParams) => {
    if (method === "computer/read" || method === "model/list") return { status: "error", error: {
      code: "serviceUnavailable", message: "Computer directory offline.", localization: null,
    } };
    if (method === "file/upload") return { status: "success", data: { file: {
      ...file, fileId: "staged", originalFilename: params.request.filename, scope: params.request.scope,
    } } };
    const modelRef = { provider: "fixture", id: "document-model" };
    const modelSelection = { modelRef, reasoningEffort: null };
    const responses: Record<string, unknown> = {
      "session/get": { session: { id: sessionId, sessionKind: "standard", nodeId, projectId, messageCount: 1, modelSelection, effectiveMode: "askForApproval" } },
      "session/active_turn": { activeTurn: null },
      "session/history": { data: [], nextCursor: null, lastStableEventId: null },
      "automation/read": { sessionId, goal: null, loops: [] },
      "automation/commandCatalog": { data: [] },
      "file/list": { data: [file], nextCursor: null },
      "file/read": { file },
    };
    const data = responses[method];
    return { status: "success", data };
  });
  const connection = useAppStore.getState().connection;
  useAppStore.getState().selectComputer(connectionScope(connection), nodeId);
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={productQueries}>{children}</QueryClientProvider>
  );
  return { http, invoke, file, wrapper, sessionId };
}

it("lists and uploads Files for a verified ordinary Worker after an offline restart", async () => {
  const { file, wrapper, invoke, http } = await offlineOrdinaryWorker(true);
  const { container } = render(<MemoryRouter><FilesPage /></MemoryRouter>, { wrapper });
  await screen.findByText(file.originalFilename);
  fireEvent.click(screen.getByRole("button", { name: "Upload" }));
  fireEvent.change(container.querySelector("input[type=file]")!, { target: { files: [new File(["notes"], "notes.txt")] } });
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("file/upload", expect.objectContaining({ request: expect.objectContaining({
    scope: { type: "personal" }, filename: "notes.txt",
  }) }), expect.anything()));
  expect(http).not.toHaveBeenCalled();
});

it("stages a project attachment in a saved standard chat without prior online Computer verification", async () => {
  const { file, wrapper, invoke, sessionId } = await offlineOrdinaryWorker(false);
  const { container } = render(<MemoryRouter><ChatSessionContent sessionId={sessionId} /></MemoryRouter>, { wrapper });
  await waitFor(() => expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(false), { timeout: 4_000 });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  fireEvent.click(screen.getByRole("button", { name: "Choose an existing file" }));
  fireEvent.click(await screen.findByText(file.originalFilename));
  fireEvent.change(container.querySelector("input[type=file]")!, { target: { files: [new File(["draft"], "draft.txt")] } });
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("file/upload", expect.objectContaining({ request: expect.objectContaining({
    scope: file.scope, filename: "draft.txt",
  }) }), expect.anything()));
  await screen.findByText("draft.txt");
  expect((screen.getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(true);
});
