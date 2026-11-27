// @vitest-environment jsdom
import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { chatKeys } from "./useChatQueries";
import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";

afterEach(cleanup);

it("coalesces activity bursts and refreshes changes arriving during the current read", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: PropsWithChildren) => createElement(QueryClientProvider, { client }, children);
  let revision = 0;
  const releases: Array<() => void> = [];
  const read = vi.fn(async () => {
    const captured = revision;
    await new Promise<void>(resolve => releases.push(resolve));
    return captured;
  });
  const { result } = renderHook(() => {
    const { connectionKey, invalidateSessionQueries } = useSessionQueryInvalidation("session-a");
    const query = useQuery({ queryKey: chatKeys.sessionHistory(connectionKey, "session-a"),
      queryFn: read, initialData: -1, staleTime: Infinity });
    return { invalidateSessionQueries, data: query.data };
  }, { wrapper });
  const refreshes: Array<Promise<void>> = [];
  act(() => { refreshes.push(result.current.invalidateSessionQueries()); });
  await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  revision = 20;
  await act(async () => {
    for (let i = 0; i < 20; i++) refreshes.push(result.current.invalidateSessionQueries());
    await Promise.resolve();
  });
  expect(read).toHaveBeenCalledTimes(1);
  act(() => { releases[0](); });
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  await act(async () => { releases[1](); await Promise.all(refreshes); });
  await waitFor(() => expect(result.current.data).toBe(20));
});

it("keeps a new Session refresh independent from the previous Session's pending read", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: PropsWithChildren) => createElement(QueryClientProvider, { client }, children);
  let release!: () => void;
  const oldRead = vi.fn(() => new Promise<number>(resolve => { release = () => resolve(1); }));
  const nextRead = vi.fn(() => Promise.resolve(2));
  const { result, rerender } = renderHook(({ sessionId }) => {
    const invalidation = useSessionQueryInvalidation(sessionId);
    useQuery({ queryKey: chatKeys.sessionHistory(invalidation.connectionKey, sessionId),
      queryFn: sessionId === "session-a" ? oldRead : nextRead, initialData: 0, staleTime: Infinity });
    return invalidation;
  }, { wrapper, initialProps: { sessionId: "session-a" } });
  let pending!: Promise<void>;
  act(() => { pending = result.current.invalidateSessionQueries(); });
  await waitFor(() => expect(oldRead).toHaveBeenCalledOnce());
  rerender({ sessionId: "session-b" });
  await act(async () => { await result.current.invalidateSessionQueries(); });
  expect(nextRead).toHaveBeenCalledOnce();
  await act(async () => { release(); await pending; });
  expect(client.getQueryData(chatKeys.sessionHistory(result.current.connectionKey, "session-b"))).toBe(2);
});
