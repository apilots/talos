// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { resetTalosClientFactory, setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { useSessionAutomation } from "./useSessionAutomation";
import { snapshotTasks } from "./model";
import { goal, loop } from "./automation.test-support";
import { apiErrorMessage, userVisibleError } from "@/shared/i18n/apiErrorMessage";
import { i18n } from "@/shared/i18n/setup";
const scope = vi.hoisted(() => ({ value: "scope-a" }));
vi.mock("@/features/computers", () => ({ useProductScope: () => scope.value }));
afterEach(() => { cleanup(); resetTalosClientFactory(); scope.value = "scope-a"; });
afterEach(async () => { await i18n.changeLanguage("en"); });
function wrapper() {
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return { cache, Wrapper: ({ children }: PropsWithChildren) => <QueryClientProvider client={cache}>{children}</QueryClientProvider> };
}
function install(automation: object) {
  setTalosClientFactory(() => ({ automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }), ...automation } }) as unknown as TalosWebClient);
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => { resolve = complete; });
  return { promise, resolve };
}
it("removes a stopped Loop immediately and after refresh without querying ended tasks", async () => {
  let active = true;
  const list = vi.fn();
  const read = vi.fn(async () => ({ sessionId: "s1", goal: null, loops: active ? [loop()] : [] }));
  const execute = vi.fn(async () => {
    active = false;
    return { effect: "applied", createdObject: null, boundOrigin: null, snapshot: await read() };
  });
  install({ read, list, execute });
  const { Wrapper } = wrapper();
  const { result } = renderHook(() => useSessionAutomation("s1", true), { wrapper: Wrapper });
  await waitFor(() => expect(result.current.tasks).toHaveLength(1));
  await act(async () => { await result.current.execute({ target: "loop", action: { type: "stop", loopId: "loop-1" } }); });
  expect(result.current.tasks).toEqual([]);
  await act(async () => { await result.current.refresh(); });
  expect(result.current.tasks).toEqual([]);
  expect(list).not.toHaveBeenCalled();
});
it.each(["active", "paused", "complete", "cleared"] as const)("shows only an effective Goal (%s)", (status) => {
  const tasks = snapshotTasks({ sessionId: "s1", goal: goal({ status }), loops: [] });
  expect(tasks).toHaveLength(status === "active" || status === "paused" ? 1 : 0);
});
it("excludes stopped Loops and keeps enabled Loops ordered", () => {
  const tasks = snapshotTasks({ sessionId: "s1", goal: null, loops: [loop({ id: "later", createdAt: 30 }), loop({ id: "ended", status: "stopped", createdAt: 10 }), loop({ id: "first", createdAt: 20 })] });
  expect(tasks.map((task) => task.value.id)).toEqual(["first", "later"]);
});
it.each(["goal", "loop"] as const)("removes a naturally ended %s on the original invalidation path", async (mode) => {
  let ended = false;
  const read = vi.fn(async () => ({ sessionId: "s1", goal: mode === "goal" ? goal({ status: ended ? "complete" : "active" }) : null, loops: mode === "loop" && !ended ? [loop()] : [] }));
  install({ read });
  const { Wrapper, cache } = wrapper();
  const { result } = renderHook(() => useSessionAutomation("s1", true), { wrapper: Wrapper });
  await waitFor(() => expect(result.current.tasks).toHaveLength(1));
  ended = true;
  await act(async () => { await cache.invalidateQueries({ queryKey: ["talos", "scope-a", "automation"] }); });
  await waitFor(() => expect(result.current.tasks).toEqual([]));
});
it("does not expose a late old Session reply after switching scope", async () => {
  const old = deferred<AppProtocol.SessionAutomationSnapshot>();
  install({ read: vi.fn().mockReturnValue(old.promise) });
  const { Wrapper } = wrapper();
  const { result, rerender } = renderHook(({ id }) => useSessionAutomation(id, true), { wrapper: Wrapper, initialProps: { id: "s1" } });
  install({ read: vi.fn().mockResolvedValue({ sessionId: "s2", goal: goal({ id: "new-goal", sessionId: "s2" }), loops: [] }) });
  scope.value = "scope-b";
  rerender({ id: "s2" });
  await waitFor(() => expect(result.current.tasks.map((task) => task.value.id)).toEqual(["new-goal"]));
  await act(async () => { old.resolve({ sessionId: "s1", goal: goal({ id: "old-goal" }), loops: [] }); });
  expect(result.current.tasks.map((task) => task.value.id)).toEqual(["new-goal"]);
});
it("surfaces read authorization failure through the original error channel", async () => {
  const failure = new TalosApiError({ code: AppProtocol.ErrorCode.PermissionDenied, message: "Denied", localization: null });
  install({ read: vi.fn().mockRejectedValue(failure) });
  const { Wrapper } = wrapper();
  const { result } = renderHook(() => useSessionAutomation("s1", true), { wrapper: Wrapper });
  await waitFor(() => expect(result.current.error).toBeTruthy());
});
it("keeps a missing-Goal command failure structured across language changes", async () => {
  install({ read: vi.fn().mockResolvedValue({ sessionId: "s1", goal: null, loops: [] }) });
  const { Wrapper } = wrapper();
  const { result } = renderHook(() => useSessionAutomation("s1", true), { wrapper: Wrapper });
  let failure: unknown;
  await act(async () => {
    failure = await result.current.command({ type: "pauseGoal" }).catch((error: unknown) => error);
  });
  const visible = userVisibleError(failure, "Automation operation failed.");
  expect(visible.code).toBe(AppProtocol.ErrorCode.ResourceNotFound);
  await i18n.changeLanguage("zh-CN");
  expect(apiErrorMessage(visible)).toBe("当前会话没有 Goal。");
  await i18n.changeLanguage("en");
  expect(apiErrorMessage(visible)).toBe("No Goal exists in this chat.");
});
