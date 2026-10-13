// Drive the actual Desktop controller, SDK transport and app-server subprocess.
import { createInterface } from "node:readline";
import { DesktopAppServerController } from "../../../talos-app/desktop/src/app-server-controller";

const controller = new DesktopAppServerController({
  command: process.env.TALOS_APP_SERVER_BIN,
  env: process.env,
  desktopHost: true,
  onRuntimeState: (state) => {
    if (state.kind === "error") {
      process.stderr.write(`Desktop runtime error: ${state.error?.code ?? "internalError"}\n`);
    }
  },
});
try {
  for await (const line of createInterface({ input: process.stdin })) {
    const request = JSON.parse(line) as {
      id: number;
      method: string;
      params: unknown;
      actorToken?: string;
    };
    try {
      const result = request.method === "connect"
        ? await controller.connect()
        : await controller.invoke(
          request.method as Parameters<DesktopAppServerController["invoke"]>[0],
          request.params,
          request.actorToken ? {
            actorCredentials: { credential: { type: "existing_session_token", token: request.actorToken } },
          } : undefined,
        );
      process.stdout.write(`${JSON.stringify({ id: request.id, result })}\n`);
    } catch (error) {
      const code = (error as { code?: string }).code ?? "internalError";
      process.stdout.write(`${JSON.stringify({ id: request.id, error: { code } })}\n`);
    }
  }
} finally {
  await controller.close();
}
