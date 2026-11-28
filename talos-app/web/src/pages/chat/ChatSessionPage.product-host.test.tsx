// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { HashRouter, Route, Routes } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { productQueries, sessionDetailKey } from "@/shared/api/product-queries";
import { sessionPath } from "@/shared/api/session-route";
import { ChatSessionPage } from "./ChatSessionPage";
import { getChatSessionPageMocks, resetChatSessionPageMocks } from "./ChatSessionPage.test-support";

function requestUrl(url: Parameters<typeof fetch>[0]): string {
  return typeof url === "string" ? url : url instanceof URL ? url.href : url.url;
}
vi.mock("@/features/chat", async importOriginal => {
  const actual = await importOriginal<typeof import("@/features/chat")>();
  const support = await import("./ChatSessionPage.test-support");
  return { ...await support.createChatFeatureMock(() => Promise.resolve(actual)),
    useModelsQuery: actual.useModelsQuery, useProjectsView: actual.useProjectsView };
});
vi.mock("@/features/computers", async importOriginal => ({
  ...await importOriginal<typeof import("@/features/computers")>(),
  useManagedWorkerStatus: () => ({ status: {} }),
}));

afterEach(() => {
  cleanup();
  productQueries.clear();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(window, "talos");
  useAppStore.setState(useAppStore.getInitialState());
  window.history.replaceState(null, "", "/");
});

it.each([
  ["personal-a", true],
  ["own-registered-uuid", false],
] as const)("keeps %s's actual embedded response on its product throughout ChatSessionPage", async (computerId, disabled) => {
  resetChatSessionPageMocks();
  const mocks = getChatSessionPageMocks();
  mocks.realSessionQuery = true;
  const wireSession = { ...mocks.session as object, id: "sess-1", nodeId: "embedded", projectId: "project-1", messageCount: 0 };
  const modelCatalog = { models: [], defaultModelSelection: { modelRef: { provider: "mock", id: "model-1" }, reasoningEffort: null } };
  const projects = { projects: [{ project_id: "project-1", nodeId: "embedded", name: "Host project", directory: "/project", status: "available", session_count: 1 }] };
  const invoke = vi.fn().mockImplementation((method: string) => Promise.resolve({ status: "success", data:
    method === "desktop/workerIdentity" ? { nodeId: "own-registered-uuid" }
      : method === "session/get" ? { session: wireSession }
        : method === "project/list" ? projects
          : method === "config/get" ? { type: "sessionMessageSendPolicy", policy: "queue" }
            : modelCatalog }));
  Object.defineProperty(window, "talos", { configurable: true, value: { invoke } });
  useAppStore.getState().setConnection({ userId: "alice", authStatus: "authenticated", authToken: "token", workerRuntimeState: { kind: "ready", message: null } });
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.getState().selectComputer(scope, "other-personal");
  const http = vi.fn<typeof fetch>().mockImplementation(url => {
    const path = requestUrl(url);
    if (computerId === "own-registered-uuid" && path.includes("/computers/")) return Promise.reject(new Error("Directory offline"));
    return Promise.resolve(Response.json({ status: "success", data: path.includes("/computers/")
      ? { nodeId: computerId, available: false, status: "enabled" }
      : path.includes("/sessions/") ? { session: wireSession }
        : path.includes("/projects") ? projects : modelCatalog }));
  });
  vi.stubGlobal("fetch", http);
  Element.prototype.scrollIntoView = vi.fn();
  window.history.replaceState(null, "", `/#${sessionPath("sess-1", computerId)}`);
  render(<QueryClientProvider client={productQueries}><HashRouter><Routes>
    <Route path="/sessions/:sessionId" element={<ChatSessionPage />} />
  </Routes></HashRouter></QueryClientProvider>);
  await waitFor(() => expect(useAppStore.getState().selectedComputers[scope]).toBe(computerId));
  await waitFor(() => expect(screen.getByTestId("chat-composer")).toHaveAttribute("data-disabled", String(disabled)));
  expect(productQueries.getQueryData(sessionDetailKey(scope, "sess-1", computerId))).toMatchObject({ session: { nodeId: computerId } });
  await waitFor(() => expect(productQueries.getQueryData(["talos", JSON.stringify([scope, computerId]), "projects"])).toMatchObject([{ nodeId: computerId }]));
  if (disabled) {
    await waitFor(() => expect(http.mock.calls.some(([url]) => requestUrl(url).includes("/v1/models"))).toBe(true));
    for (const [url, init] of http.mock.calls.filter(([url]) => /\/v1\/(models|projects)/.test(requestUrl(url)))) {
      expect(new Headers(init?.headers).get("x-talos-computer"), requestUrl(url)).toBe(computerId);
    }
  } else {
    expect(invoke).toHaveBeenCalledWith("model/list", expect.objectContaining({ nodeId: computerId }), expect.anything());
  }
});
