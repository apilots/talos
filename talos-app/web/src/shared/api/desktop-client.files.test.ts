import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient files", () => {
  it("exposes the existing app-server files API in desktop mode", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const file = {
      fileId: "file-1",
      scope: { type: "personal" as const },
      originalFilename: "notes.docx",
      mimeType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      sizeBytes: 2,
      tags: [],
      metadata: {},
      createdAt: 1,
      updatedAt: 1,
    };
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
      calls.push({ method, params });
      if (method === "file/upload") {
        return { file } as T;
      }
      if (method === "file/list") {
        return { data: [file], nextCursor: null } as T;
      }
      throw new Error(`unexpected method: ${method}`);
    });
    const client = createDesktopTalosClient(bridge);

    await expect(
      client.files.upload(
        {
          filename: file.originalFilename,
          scope: file.scope,
          tags: [],
          metadata: {},
        },
        new Uint8Array([1, 2]),
      ),
    ).resolves.toEqual({ file });
    await expect(client.files.list({ limit: 10 })).resolves.toEqual({
      data: [file],
      nextCursor: null,
    });

    expect(calls).toEqual([
      {
        method: "file/upload",
        params: {
          request: {
            filename: file.originalFilename,
            scope: file.scope,
            tags: [],
            metadata: {},
          },
          dataBase64: "AQI=",
        },
      },
      {
        method: "file/list",
        params: { limit: 10 },
      },
    ]);
  });
});

function bridgeWithInvoke(invoke: DesktopRendererBridge["invoke"]): DesktopRendererBridge {
  return {
    invoke,
    onEvent: () => () => undefined,
    onSubscriptionTerminated: () => () => undefined,
  };
}
