import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { expect, it } from "vitest";
import { JsonRpcStdioTransport } from "./stdio";

it("allows the host to finish Worker cleanup on stdin EOF before process exit", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "talos-stdio-shutdown-"));
  const marker = path.join(directory, "cleanup.txt");
  const transport = new JsonRpcStdioTransport({
    command: process.execPath,
    args: ["-e", `const fs = require("node:fs");
      process.on("SIGTERM", () => process.exit(9));
      process.stdin.resume();
      process.stdin.on("end", () => {
        fs.writeFileSync(process.argv[1], "Worker unregistered");
        process.exit(0);
      });`, marker],
  });
  const exits: Array<{ code: number | null; signal: NodeJS.Signals | null }> = [];
  transport.onExit((status) => exits.push(status));
  try {
    transport.start();
    await Promise.all([transport.close(), transport.close()]);
    expect(await readFile(marker, "utf8")).toBe("Worker unregistered");
    expect(exits).toEqual([{ code: 0, signal: null }]);
  } finally {
    await transport.close();
    await rm(directory, { recursive: true, force: true });
  }
});

it("reports a failed host cleanup instead of resolving shutdown successfully", async () => {
  const transport = new JsonRpcStdioTransport({ command: process.execPath,
    args: ["-e", 'process.stdin.resume(); process.stdin.on("end", () => process.exit(1));'],
  });
  transport.start();
  await expect(transport.close()).rejects.toThrow("App-server exited before graceful shutdown completed.");
});
