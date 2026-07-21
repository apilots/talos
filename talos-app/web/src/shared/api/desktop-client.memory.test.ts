import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("desktop client memory transport", () => {
  it("keeps memory search and get response envelopes aligned with the HTTP client", async () => {
    const bridge = bridgeWithResponses({
      "memory/search": {
        data: [
          {
            memoryId: "mem-1",
            title: "Updated memory",
            preview: "Updated memory",
            kind: "fact",
            importance: "normal",
            occurredAt: 1,
            rank: 1,
            source: {
              date: "2026-06-24",
              sessionId: null,
              fromSeq: null,
              untilSeq: null,
            },
          },
        ],
      },
      "memory/get": {
        memory: {
          memoryId: "mem-1",
          title: "Updated memory",
          body: "Complete updated memory",
          kind: "fact",
          importance: "normal",
          occurredAt: 1,
          source: {
            date: "2026-06-24",
            sessionId: null,
            fromSeq: null,
            untilSeq: null,
          },
        },
      },
    });
    const client = createDesktopTalosClient(bridge);

    await expect(client.memory.search({
      query: "Updated",
      limit: 5,
      sinceAt: 1,
      untilAt: 2,
    })).resolves.toMatchObject({ data: [{ source: { date: "2026-06-24" } }] });
    await expect(client.memory.getEntry({ memoryId: "mem-1" })).resolves.toMatchObject({
      memory: { memoryId: "mem-1", body: "Complete updated memory" },
    });
  });
});

function bridgeWithResponses(
  responses: Partial<Record<AppProtocol.ClientMethod, unknown>>,
): DesktopRendererBridge {
  return {
    invoke: async function <T>(method: AppProtocol.ClientMethod): Promise<T> {
      if (Object.prototype.hasOwnProperty.call(responses, method)) {
        return responses[method] as T;
      }
      throw new Error(`unexpected method: ${method}`);
    },
    onEvent: () => () => undefined,
    onSubscriptionTerminated: () => () => undefined,
  };
}
