// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { DesktopBrowserState } from "@talos/sdk-app-server/desktop";
import { useDesktopBrowserWorkspace } from "./desktop-browser";
import { useAppStore } from "@/shared/store/app-store";
const fixture = vi.hoisted(() => ({ request: vi.fn(), receive: undefined as ((state: DesktopBrowserState) => void) | undefined }));
vi.mock("@/shared/api/desktop-client", () => { const browserWorkspace = { request: fixture.request, onState: (receive: typeof fixture.receive) => { fixture.receive = receive;return () => { fixture.receive = undefined; }; } };return { getDesktopBridge: () => ({ browserWorkspace }) }; });
const state = (revision: number, revealTabId: string | null = null): DesktopBrowserState => ({ sessionId: "session", revision, revealTabId, error: null, tabs: [{ id: "1", url: "https://example.test/", title: "Native page", back: false, forward: false, loading: false }] });
beforeEach(() => {
  fixture.request.mockReset();fixture.request.mockResolvedValue({ status: "success", data: state(1) });
  useAppStore.getState().setConnection({ userId: "alice", authStatus: "authenticated", authToken: "actor-token" });
  useAppStore.setState({ desktopBrowser: null, workspaceWindows: [], workspaceOpen: false, workspaceActiveKey: null });
});
afterEach(() => { cleanup(); });
it("binds with actor credentials, silently receives Agent tabs and withdraws on scope change", async() => {
  const { unmount } = renderHook(useDesktopBrowserWorkspace);
  await waitFor(() => expect(useAppStore.getState().workspaceWindows).toHaveLength(1));
  expect(fixture.request).toHaveBeenCalledWith({ kind: "bind", actorCredentials: { credential: { type: "existing_session_token", token: "actor-token" } } });
  expect(useAppStore.getState().workspaceOpen).toBe(false);
  act(() => fixture.receive?.(state(2, "1")));expect(useAppStore.getState().workspaceOpen).toBe(true);
  act(() => fixture.receive?.(state(1)));expect(useAppStore.getState().desktopBrowser?.revision).toBe(2);
  unmount();expect(fixture.request).toHaveBeenLastCalledWith({ kind: "unbind", sessionId: "session" });
  expect(useAppStore.getState().workspaceWindows).toEqual([]);
});
