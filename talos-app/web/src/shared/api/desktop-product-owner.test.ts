import { QueryClient } from "@tanstack/react-query";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { describe, expect, it, vi } from "vitest";

import type { TalosWebClient } from "./client";
import { desktopProductOwner, MANAGEMENT_LOOKUP_TIMEOUT_MS } from "./desktop-product-owner";
import { managedWorkerKey, sessionDetailKey } from "./product-queries";

const instance = { nodeId: "managed-node", conversationSessionId: "canonical" };
const missing = new TalosApiError({ code: AppProtocol.ErrorCode.ResourceNotFound, localization: null, message: "No session." });
const session = (id: string, nodeId: string, sessionKind: AppProtocol.SessionInfo["sessionKind"] = "standard") => ({ session: { id, nodeId, sessionKind } });

function endpoint() {
  return {
    sessions: { get: vi.fn().mockRejectedValue(missing) },
    computers: {
      read: vi.fn(({ nodeId }: { nodeId: string }) => Promise.resolve(({
        nodeId, managedWorkerPhase: nodeId === instance.nodeId ? "ready" : null,
      }))),
      readManagedWorker: vi.fn().mockResolvedValue({ instance }),
    },
  };
}

function setup() {
  const local = endpoint();
  const cloud = endpoint();
  const queries = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const connect = (canUseTalo = true) => desktopProductOwner(local as unknown as TalosWebClient, cloud as unknown as TalosWebClient, queries, "alice", canUseTalo, true);
  return { local, cloud, queries, owner: connect(), connect };
}

describe("Desktop session ownership", () => {
  it("admits the cold canonical conversation before its first Turn creates a session", async () => {
    const { owner, cloud } = setup();
    await expect(owner.resolve({ sessionId: instance.conversationSessionId })).resolves.toBe(cloud);
  });

  it.each([
    { name: "additional live", current: instance, nodeId: instance.nodeId },
    { name: "replaced", current: instance, nodeId: "retired-node" },
    { name: "deleted", current: null, nodeId: "retired-node" },
  ])("retains the Server as owner after a cold read of a $name Talo session", async ({ nodeId, current }) => {
    const { owner, local, cloud, queries, connect } = setup();
    cloud.computers.readManagedWorker.mockResolvedValue({ instance: current });
    const info = session("project-chat", nodeId, "talo").session;
    if (nodeId === "retired-node") {
      local.sessions.get.mockResolvedValue({ session: { ...info, title: "Earlier synced title" } });
      cloud.computers.read.mockRejectedValue(missing);
    }
    const remote = { session: { ...info, title: "Latest Server title" } };
    cloud.sessions.get.mockResolvedValue(remote);
    const opened = await owner.readSession("project-chat");
    expect(opened).toEqual({ ...remote, owner: "server" });
    queries.setQueryData(sessionDetailKey("alice", "project-chat"), opened);
    await expect(connect(false).resolve({ sessionId: "project-chat" })).resolves.toBe(cloud);
  });

  it("opens a synchronized ordinary conversation with the Server unavailable", async () => {
    const { owner, local, cloud } = setup();
    const saved = session("ordinary", "personal-node");
    local.sessions.get.mockResolvedValue(saved);
    cloud.computers.read.mockRejectedValue(new TypeError("Server unreachable"));
    await expect(owner.readSession("ordinary")).resolves.toEqual({ ...saved, owner: "desktop" });
    await expect(owner.resolve({ sessionId: "ordinary", readOnly: true })).resolves.toBe(local);
    expect(cloud.computers.read).not.toHaveBeenCalled();
  });

  it.each([false, true])("prefers confirmed canonical ownership over an old Desktop projection (cached=%s)", async (cached) => {
    const { owner, local, cloud, queries } = setup();
    const upgraded = session(instance.conversationSessionId, instance.nodeId, "talo");
    cloud.sessions.get.mockResolvedValue(upgraded);
    const oldProjection = session(instance.conversationSessionId, instance.nodeId);
    local.sessions.get.mockResolvedValue(oldProjection);
    if (cached) queries.setQueryData(sessionDetailKey("alice", instance.conversationSessionId), { ...oldProjection, owner: "desktop" });
    queries.setQueryData(managedWorkerKey("alice"), { instance });
    await expect(owner.readSession(instance.conversationSessionId)).resolves.toEqual({ ...upgraded, owner: "server" });
    await expect(owner.resolve({ sessionId: instance.conversationSessionId, nodeId: instance.nodeId })).resolves.toBe(cloud);
  });

  it("surfaces an unavailable Talo owner instead of presenting stale synced detail", async () => {
    const { owner, local, cloud } = setup();
    local.sessions.get.mockResolvedValue(session("archived", "retired-node", "talo"));
    const offline = new TypeError("Server unreachable");
    cloud.sessions.get.mockRejectedValue(offline);
    await expect(owner.readSession("archived")).rejects.toBe(offline);
  });

  it("routes each session independently of other Computer operations", async () => {
    const { owner, local, cloud } = setup();
    local.sessions.get.mockResolvedValueOnce(session("ordinary", "personal-node"));
    await expect(owner.resolve({ nodeId: instance.nodeId })).resolves.toBe(cloud);
    await expect(owner.resolve({ sessionId: "ordinary" })).resolves.toBe(local);
    await expect(owner.resolve({ sessionId: instance.conversationSessionId })).resolves.toBe(cloud);
  });

  it("does not treat an unverified local Computer cache entry with null phase as proof of ordinary ownership", async () => {
    const { owner, cloud, queries } = setup();
    queries.setQueryData(["talos", "alice", "computer", instance.nodeId], {
      nodeId: instance.nodeId,
      managedWorkerPhase: null,
    });
    await expect(owner.resolve({ nodeId: instance.nodeId })).resolves.toBe(cloud);
    expect(cloud.computers.read).toHaveBeenCalledWith({ nodeId: instance.nodeId }, expect.any(AbortSignal));
  });

  it("bounds stalled management Server Computer classification with a typed timeout and aborts the request", async () => {
    vi.useFakeTimers();
    try {
      const { owner, local, cloud } = setup();
      const signals: AbortSignal[] = [];
      cloud.computers.read.mockImplementation((_req: { nodeId: string }, signal?: AbortSignal) => {
        if (signal) signals.push(signal);
        return new Promise(() => {});
      });

      const readOnlyRoute = owner.resolve({ nodeId: "stalled-read-only", readOnly: true });
      await vi.advanceTimersByTimeAsync(MANAGEMENT_LOOKUP_TIMEOUT_MS);
      await expect(readOnlyRoute).resolves.toBe(local);
      expect(signals[0]?.aborted).toBe(true);

      const executionRoute = owner.resolve({ nodeId: "stalled-execution" });
      const rejection = expect(executionRoute).rejects.toMatchObject({
        code: AppProtocol.ErrorCode.Timeout,
      });
      await vi.advanceTimersByTimeAsync(MANAGEMENT_LOOKUP_TIMEOUT_MS);
      await rejection;
      expect(signals[1]?.aborted).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});
