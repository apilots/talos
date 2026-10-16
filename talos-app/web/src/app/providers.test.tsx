// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, act } from "@testing-library/react";
import { useQueries, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { afterEach, expect, it, vi } from "vitest";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { llmConfigKey, webSearchConfigKey, profileKey } from "@/features/settings";
import { AppProviders } from "./providers";

vi.mock("@/features/auth", () => ({ useAuthBootstrap: () => undefined }));
vi.mock("@/app/router", () => ({ router: {} }));
vi.mock("react-router-dom", () => ({ RouterProvider: () => <ActorSettings /> }));

let client: QueryClient;
const pending: Array<() => void> = [];
function settingsKeys(scope: string) {
  const environment = JSON.stringify([scope, "worker"]);
  return [llmConfigKey(environment), webSearchConfigKey(environment), profileKey(scope)];
}
function ActorSettings() {
  client = useQueryClient();
  const connection = useAppStore((state) => state.connection);
  const results = useQueries({ queries: settingsKeys(connectionScope(connection)).map((queryKey) => ({
    queryKey, queryFn: () => new Promise<string>((resolve) => {
      if (connection.userId === "alice") resolve("alice");
      else pending.push(() => resolve(connection.userId));
    }),
  })) });
  return <p>{results.map((query) => query.data ?? "pending").join(":")}</p>;
}
afterEach(() => { cleanup(); client.clear(); pending.length = 0; });

it("clears old actor settings without cancelling the new actor's first requests", async () => {
  useAppStore.getState().setConnection({ controlPlaneEndpoint: "https://agent.talos.love", userId: "alice" });
  const oldKeys = settingsKeys(connectionScope(useAppStore.getState().connection));
  render(<AppProviders />);
  await screen.findByText("alice:alice:alice");
  act(() => useAppStore.getState().setConnection({ userId: "bob" }));
  await waitFor(() => expect(pending.length).toBeGreaterThanOrEqual(3));
  await act(async () => { pending.forEach((resolve) => resolve()); });
  await screen.findByText("bob:bob:bob");
  await waitFor(() => {
    for (const key of oldKeys) expect(client.getQueryState(key)).toBeUndefined();
    for (const key of settingsKeys(connectionScope(useAppStore.getState().connection))) expect(client.getQueryData(key)).toBe("bob");
  });
});
