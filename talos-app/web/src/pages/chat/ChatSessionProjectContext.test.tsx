// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { MemoryRouter, Route, Routes } from "react-router-dom";

import "@/shared/i18n/setup";
import { AppSessionContext } from "@/app/layouts/AppTopBar";
import { chatKeys } from "@/features/chat";
import { ChatSessionPage } from "./ChatSessionPage";
import { getChatSessionPageMocks } from "./ChatSessionPage.test-support";

const api = vi.hoisted(() => ({ listProjects: vi.fn() }));
vi.mock("@/features/chat/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/features/chat/api")>()),
  listProjects: api.listProjects,
}));
vi.mock("@/features/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/chat")>();
  const support = await import("./ChatSessionPage.test-support");
  return { ...(await support.createChatFeatureMock(importOriginal)),
    useProjectsQuery: actual.useProjectsQuery, useProjectsView: actual.useProjectsView };
});
vi.mock("@/shared/store/app-store", async () => {
  const support = await import("./ChatSessionPage.test-support");
  return support.createAppStoreMock();
});
vi.mock("@/features/computers", () => ({
  useComputers: () => ({ selectedId: "other-computer" }),
  useSelectedComputerId: () => "other-computer",
  useProductScope: () => "test-scope",
  useComputer: () => ({ data: { available: true } }),
}));

afterEach(cleanup);

it("refreshes an externally created bound Project into the shared header cache without reload", async () => {
  Element.prototype.scrollIntoView = vi.fn();
  const mocks = getChatSessionPageMocks();
  mocks.session = { title: "Analyze product", nodeId: "session-computer", projectId: null };
  mocks.sessionHistoryIsSuccess = true;
  mocks.sessionHistoryError = undefined;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  const key = chatKeys.projects(JSON.stringify(["test-scope", "session-computer"]));
  queryClient.setQueryData(key, []);
  api.listProjects.mockResolvedValue([
    { id: "external-project", name: "External Project", directory: "/tmp/project" },
  ]);
  const content = () => <QueryClientProvider client={queryClient}>
    <MemoryRouter initialEntries={["/sessions/session-1"]}>
      <header><AppSessionContext /></header>
      <Routes><Route path="/sessions/:sessionId" element={<ChatSessionPage />} /></Routes>
    </MemoryRouter>
  </QueryClientProvider>;
  const view = render(content());
  expect(screen.queryByRole("button", { name: "External Project" })).toBeNull();
  expect(api.listProjects).not.toHaveBeenCalled();

  // A different client creates the Project and its Session on the same Computer.
  mocks.session = { title: "Analyze product", nodeId: "session-computer", projectId: "external-project" };
  view.rerender(content());

  await waitFor(() => expect(screen.getByRole("button", { name: "External Project" })).toBeTruthy());
  expect(screen.getByTitle("Analyze product")).toBeTruthy();
  expect(api.listProjects).toHaveBeenCalledExactlyOnceWith("session-computer");
  expect(queryClient.getQueryData(key)).toEqual([
    { id: "external-project", name: "External Project", directory: "/tmp/project" },
  ]);
  queryClient.clear();
});
