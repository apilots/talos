// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter, HashRouter, Link, Route, Routes, useNavigate, useParams } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { useSessionQuery } from "@/features/chat/hooks/useChatQueries";
import { getSession, listModels, listProjects, listSessions, updateSessionPermissionMode } from "@/features/chat/api";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { getTalosClient } from "./client";
import { managedWorkerKey, productQueries, sessionDetailKey } from "./product-queries";
import { sessionPath, sessionRouteComputer } from "./session-route";

function requestUrl(url: Parameters<typeof fetch>[0] | undefined): string {
  return typeof url === "string" ? url : url instanceof URL ? url.href : url?.url ?? "";
}
afterEach(() => {
  cleanup();
  productQueries.clear();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(window, "talos");
  useAppStore.setState(useAppStore.getInitialState());
  window.history.replaceState(null, "", "/");
});

function account(desktop: boolean) {
  const invoke = vi.fn().mockResolvedValue({ status: "success", data: { nodeId: "local" } });
  if (desktop) Object.defineProperty(window, "talos", { configurable: true, value: { invoke } });
  useAppStore.getState().setConnection({ userId: "alice", authStatus: "authenticated", authToken: "token" });
  const scope = connectionScope(useAppStore.getState().connection);
  useAppStore.getState().selectComputer(scope, "computer-b");
  const http = vi.fn<typeof fetch>().mockImplementation((_url, init) => {
    const computer = new Headers(init?.headers).get("x-talos-computer");
    return Promise.resolve(Response.json({ status: "success", data: {
      session: { id: "session", nodeId: computer === "talo" ? "talo" : "embedded", title: `Session on ${computer}` },
    } }));
  });
  vi.stubGlobal("fetch", http);
  return { scope, invoke, http };
}

function SessionNavigation({ scope }: { scope: string }) {
  const sessionId = useParams().sessionId!;
  const session = useSessionQuery(sessionId);
  const navigate = useNavigate();
  return <>
    <h1>{session.data?.title ?? "Loading"}</h1>
    <output data-testid="session-computer">{session.data?.nodeId}</output>
    <Link to={sessionPath(sessionId, session.data?.nodeId)}>Reopen Session</Link>
    <button onClick={() => void listModels(session.data?.nodeId)}>Models</button>
    <Link to={sessionPath("session", "computer-b")} onClick={() => useAppStore.getState().selectComputer(scope, "computer-b")}>Computer B</Link>
    <button onClick={() => void navigate(-1)}>Back</button>
  </>;
}

it.each([false, true])("returns A to its product after A → B → back with no metadata cache (Desktop=%s)", async desktop => {
  const { scope, http } = account(desktop);
  window.history.replaceState(null, "", `${desktop ? "/#" : ""}${sessionPath("session", "computer-a")}`);
  const Router = desktop ? HashRouter : BrowserRouter;
  render(<QueryClientProvider client={productQueries}><Router><Routes>
    <Route path="/sessions/:sessionId" element={<SessionNavigation scope={scope} />} />
  </Routes></Router></QueryClientProvider>);
  expect(await screen.findByRole("heading", { name: "Session on computer-a" })).toBeInTheDocument();
  expect(screen.getByTestId("session-computer")).toHaveTextContent("computer-a");
  expect(screen.getByRole("link", { name: "Reopen Session" })).toHaveAttribute("href", `${desktop ? "#" : ""}/sessions/session?computerId=computer-a`);
  fireEvent.click(screen.getByRole("button", { name: "Models" }));
  await waitFor(() => expect(requestUrl(http.mock.calls.at(-1)?.[0])).toContain("/v1/models"));
  expect(new Headers(http.mock.calls.at(-1)?.[1]?.headers).get("x-talos-computer")).toBe("computer-a");
  fireEvent.click(screen.getByRole("link", { name: "Computer B" }));
  expect(await screen.findByRole("heading", { name: "Session on computer-b" })).toBeInTheDocument();
  expect(productQueries.getQueryData(sessionDetailKey(scope, "session", "computer-a"))).toMatchObject({ session: { nodeId: "computer-a" } });
  expect(productQueries.getQueryData(sessionDetailKey(scope, "session", "computer-b"))).toMatchObject({ session: { nodeId: "computer-b" } });
  // A background refresh must retain the product captured by its original query.
  await act(() => productQueries.refetchQueries({ queryKey: sessionDetailKey(scope, "session", "computer-a"), type: "all" }));
  expect(productQueries.getQueryData(sessionDetailKey(scope, "session", "computer-a"))).toMatchObject({ session: { nodeId: "computer-a" } });
  act(() => productQueries.clear());
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  await waitFor(() => expect(sessionRouteComputer("session")).toBe("computer-a"));
  expect(await screen.findByRole("heading", { name: "Session on computer-a" })).toBeInTheDocument();
  expect(new Headers(http.mock.calls.at(-1)?.[1]?.headers).get("x-talos-computer")).toBe("computer-a");
});

it.each([false, true])("opens and refreshes an explicit deep link before metadata is cached (Desktop=%s)", async desktop => {
  const { http } = account(desktop);
  window.history.replaceState(null, "", desktop ? "/#/sessions/session?computerId=computer-a" : sessionPath("session", "computer-a"));
  await getSession("session");
  productQueries.clear();
  await getSession("session");
  expect(http.mock.calls.map(([, init]) => new Headers(init?.headers).get("x-talos-computer"))).toEqual(["computer-a", "computer-a"]);
});

it.each([false, true])("uses known Talo ownership while a personal Computer is selected (Desktop=%s)", async desktop => {
  const { scope, http } = account(desktop);
  productQueries.setQueryData(managedWorkerKey(scope), { instance: { nodeId: "talo", conversationSessionId: "managed" } });
  await getSession("managed");
  await updateSessionPermissionMode("managed", "approveForMe");
  expect(http.mock.calls.map(([, init]) => new Headers(init?.headers).get("x-talos-computer"))).toEqual(["talo", "talo"]);
});

it("uses an explicit URL over cached or caller targets without probing another owner", async () => {
  const { scope, http } = account(true);
  productQueries.setQueryData(sessionDetailKey(scope, "session"), { session: { id: "session", nodeId: "computer-b" } });
  window.history.replaceState(null, "", sessionPath("session", "computer-a"));
  await (await getTalosClient({ sessionId: "session", nodeId: "computer-b" })).sessions.get({ sessionId: "session" });
  expect(http).toHaveBeenCalledOnce();
  expect(new Headers(http.mock.calls[0][1]?.headers).get("x-talos-computer")).toBe("computer-a");
});

it("keeps retained local history on its original Host and preserves its read-only rejection", async () => {
  const { invoke, http } = account(true);
  window.history.replaceState(null, "", sessionPath("legacy", "embedded"));
  invoke.mockImplementation((method: string) => Promise.resolve(method === "session/get"
    ? { status: "success", data: { session: { id: "legacy", nodeId: "embedded", title: "Retained history" } } }
    : { status: "error", error: { code: "invalidRequest", message: "This Session is read-only.", localization: null } }));
  expect((await getSession("legacy")).session.title).toBe("Retained history");
  await expect(updateSessionPermissionMode("legacy", "approveForMe")).rejects.toMatchObject({ code: "invalidRequest" });
  expect(http).not.toHaveBeenCalled();
});

it("does not use a route's Computer for a different Session", () => {
  expect(sessionRouteComputer("other", { pathname: "/sessions/session", search: "?computerId=computer-a", hash: "" })).toBeUndefined();
});

it.each([false, true])("keeps personal Session and Project display bindings outside the Host alias (Desktop=%s)", async desktop => {
  const { http } = account(desktop);
  http.mockImplementation(url => Promise.resolve(Response.json({ status: "success", data: requestUrl(url).includes("/projects")
    ? { projects: [{ project_id: "project", nodeId: "embedded", name: "Project", directory: "/project" }] }
    : { sessions: [{ id: "session", nodeId: "embedded" }] } })));
  expect((await listSessions("computer-a"))[0].nodeId).toBe("computer-a");
  expect((await listProjects("computer-a"))[0].nodeId).toBe("computer-a");
});

it("keeps the current Host's registered UUID on local detail and mutation responses", async () => {
  const { invoke, http } = account(true);
  invoke.mockImplementation((method: string) => Promise.resolve({ status: "success", data: method === "desktop/workerIdentity"
    ? { nodeId: "own-registered-uuid" }
    : { session: { id: "session", nodeId: "embedded" } } }));
  window.history.replaceState(null, "", sessionPath("session", "own-registered-uuid"));
  expect((await getSession("session")).session.nodeId).toBe("own-registered-uuid");
  expect((await updateSessionPermissionMode("session", "approveForMe")).nodeId).toBe("own-registered-uuid");
  expect(http).not.toHaveBeenCalled();
});

it("preserves a cloud Worker's actual UUID in detail, list and Project responses", async () => {
  const { http } = account(true);
  http.mockImplementation(() => Promise.resolve(Response.json({ status: "success", data: {
    session: { id: "session", nodeId: "cloud-worker-uuid" },
    sessions: [{ id: "session", nodeId: "cloud-worker-uuid" }],
    projects: [{ project_id: "project", nodeId: "cloud-worker-uuid", name: "Project", directory: "/project" }],
  } })));
  expect((await getSession("session", "cloud-worker-uuid")).session.nodeId).toBe("cloud-worker-uuid");
  expect((await listSessions("cloud-worker-uuid"))[0].nodeId).toBe("cloud-worker-uuid");
  expect((await listProjects("cloud-worker-uuid"))[0].nodeId).toBe("cloud-worker-uuid");
});
