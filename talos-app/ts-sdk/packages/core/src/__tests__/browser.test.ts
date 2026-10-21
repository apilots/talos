import { expect, it, vi } from "vitest";
import { TalosClient } from "../client";
import { AppServerClient } from "../app-server";
import { RecordingTransport } from "./app-server.fixture";

it("keeps Browser commands typed and bound to the selected Computer in both transports", async () => {
  const request = vi.fn().mockResolvedValue({ enabled: false });
  const client = new TalosClient({ request }, { stream: vi.fn() }, "http://localhost:8080");
  const params = { nodeId: "worker-1", operation: { kind: "pause" as const, expectedRevision: 7 } };
  await client.browser.operate(params);
  expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/browser", { method: "POST", body: JSON.stringify(params) });
  const transport = new RecordingTransport();
  const desktop = new AppServerClient(transport);
  await desktop.browser(params);
  expect(transport.requests).toContainEqual({ method: "browser/operate", params });
});
