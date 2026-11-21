import { QueryClient } from "@tanstack/react-query";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { describe, expect, it, vi } from "vitest";

import type { TalosWebClient } from "./client";
import { desktopProductOwner } from "./desktop-product-owner";
import { sessionDetailKey } from "./product-queries";

const instance = { nodeId: "managed-node", conversationSessionId: "canonical" };
const missing = new TalosApiError({ code: AppProtocol.ErrorCode.ResourceNotFound, localization: null, message: "No session." });
const session = (id: string, nodeId: string) => ({ session: { id, nodeId } });

function endpoint() {
  return {
    sessions: { get: vi.fn().mockRejectedValue(missing) },
    computers: {
      read: vi.fn(async ({ nodeId }: { nodeId: string }) => ({
        nodeId, managedWorkerPhase: nodeId === instance.nodeId ? "ready" : null,
      })),
      readManagedWorker: vi.fn().mockResolvedValue({ instance }),
    },
  };
}

function setup() {
  const local = endpoint();
  const cloud = endpoint();
  const queries = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const connect = () => desktopProductOwner(local as unknown as TalosWebClient, cloud as unknown as TalosWebClient, queries, "alice", true);
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
    const info = session("project-chat", nodeId).session;
    if (nodeId === "retired-node") {
      local.sessions.get.mockResolvedValue({ session: { ...info, title: "Earlier synced title" } });
      cloud.computers.read.mockRejectedValue(missing);
    }
    const remote = { session: { ...info, title: "Latest Server title" } };
    cloud.sessions.get.mockResolvedValue(remote);
    const opened = await owner.readSession("project-chat");
    expect(opened).toMatchObject({ ...remote, owner: "server" });
    queries.setQueryData(sessionDetailKey("alice", "project-chat"), opened);
    await expect(connect().resolve({ sessionId: "project-chat", readOnly: true })).resolves.toBe(cloud);
  });

  it("opens a synchronized ordinary conversation with the Server unavailable", async () => {
    const { owner, local, cloud } = setup();
    const saved = session("ordinary", "personal-node");
    local.sessions.get.mockResolvedValue(saved);
    cloud.computers.read.mockRejectedValue(new TypeError("Server unreachable"));
    await expect(owner.readSession("ordinary")).resolves.toMatchObject({ ...saved, owner: "desktop" });
    await expect(owner.resolve({ sessionId: "ordinary", readOnly: true })).resolves.toBe(local);
  });

  it("recovers an explicit local draft whose first Turn was admitted during a Server outage", async () => {
    const { owner, local, cloud } = setup();
    cloud.sessions.get.mockRejectedValue(new TypeError("Server offline"));
    await expect(owner.readSession("draft", "embedded")).rejects.toBe(missing);
    const saved = session("draft", "embedded");
    local.sessions.get.mockResolvedValue(saved);
    await expect(owner.readSession("draft", "embedded")).resolves.toMatchObject({ ...saved, owner: "desktop" });
    expect(cloud.sessions.get).not.toHaveBeenCalled();
  });

  it("routes a session by its own Computer across other Computer operations", async () => {
    const { owner, local, cloud } = setup();
    local.sessions.get.mockResolvedValueOnce(session("ordinary", "personal-node"));
    await expect(owner.resolve({ nodeId: instance.nodeId })).resolves.toBe(cloud);
    await expect(owner.resolve({ sessionId: "ordinary" })).resolves.toBe(local);
    await expect(owner.resolve({ sessionId: instance.conversationSessionId })).resolves.toBe(cloud);
  });
});
