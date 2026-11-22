import { createServer } from "node:http";
import { once } from "node:events";
import { expect, it, vi } from "vitest";
import { createBrowserTalosClient, type FileUploadProgress } from "@talos/sdk-browser";
import { DESKTOP_CLOUD_ORIGIN } from "@talos/sdk-app-server/desktop";
import type { DesktopAppServerController } from "./app-server-controller";
import { controlPlaneFetch } from "./control-plane";

it("reports the encoded bytes delivered through the fixed Desktop Server transport", async () => {
  let receivedBytes = 0;
  let receivedFile = "";
  let authorization: string | undefined;
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) chunks.push(Buffer.from(chunk));
    const bytes = Buffer.concat(chunks);
    receivedBytes = bytes.byteLength;
    authorization = request.headers.authorization;
    const form = await new Response(bytes, { headers: { "content-type": request.headers["content-type"]! } }).formData();
    receivedFile = await (form.get("file") as Blob).text();
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ status: "success", data: { file: {} } }));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  try {
    const address = server.address();
    if (!address || typeof address === "string") throw new Error("The local Server did not bind a TCP port.");
    const invoke = vi.fn().mockResolvedValue({ controlPlaneEndpoint: `http://127.0.0.1:${address.port}`, controlPlaneTrustedCaPem: null });
    const client = createBrowserTalosClient({ baseUrl: DESKTOP_CLOUD_ORIGIN,
      fetch: controlPlaneFetch({ invoke } as unknown as DesktopAppServerController), getToken: () => "actor-token" });
    const progress: FileUploadProgress[] = [];

    await client.files.upload({ filename: "large.txt", scope: { type: "personal" }, tags: [], metadata: {} },
      new Blob(["upload".repeat(32_768)]), { onProgress: value => progress.push(value) });

    expect(receivedFile).toBe("upload".repeat(32_768));
    expect(authorization).toBe("Bearer actor-token");
    expect(progress.length).toBeGreaterThan(2);
    expect(progress.at(-1)).toEqual({ loadedBytes: receivedBytes, totalBytes: receivedBytes });
    expect(invoke).toHaveBeenCalledWith("desktop/workerIdentity", null);
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  }
});
