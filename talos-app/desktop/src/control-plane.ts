import { getCACertificates } from "node:tls";
import { Agent } from "undici";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { DESKTOP_CLOUD_ORIGIN } from "@talos/sdk-app-server/desktop";
import type { DesktopAppServerController } from "./app-server-controller";

export function trustedServerUrl(value: string): URL {
  const url = new URL(value);
  const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
  if (url.username || url.password || (url.protocol !== "https:" && !(url.protocol === "http:" && loopback))) {
    throw new Error("The Desktop Server URL must use HTTPS or loopback HTTP without credentials.");
  }
  return url;
}

/** The shared SDK supplies the API path, body and credentials. The host supplies the destination and TLS trust. */
export function controlPlaneFetch(runtime: DesktopAppServerController): typeof fetch {
  let configured: Promise<{ origin: URL; dispatcher?: Agent }> | undefined;
  function connection() {
    return configured ??= runtime.invoke<AppProtocol.DesktopWorkerIdentityResponse>("desktop/workerIdentity", null)
      .then(identity => {
        if (!identity.controlPlaneEndpoint) throw new Error("The Desktop control plane is unavailable.");
        const origin = trustedServerUrl(identity.controlPlaneEndpoint);
        const ca = identity.controlPlaneTrustedCaPem;
        // Deployments may trust their self-signed server certificate directly.
        const dispatcher = ca ? new Agent({ connect: { ca: [...getCACertificates("default"), ca], allowPartialTrustChain: true } }) : undefined;
        return { origin, dispatcher };
      }).catch(error => { configured = undefined; throw error; });
  }
  return async (input, init) => {
    const request = new Request(input, init);
    const url = new URL(request.url);
    if (url.protocol + "//" + url.host !== DESKTOP_CLOUD_ORIGIN || !url.pathname.startsWith("/v1/")) {
      throw new Error("Invalid Desktop control-plane resource.");
    }
    try {
      const { origin, dispatcher } = await connection();
      const target = new URL(origin.origin);
      target.pathname = url.pathname;
      target.search = url.search;
      const headers = new Headers();
      for (const name of ["accept", "authorization", "content-type", "idempotency-key", "last-event-id", "range",
        "x-talos-computer", "x-talos-source-peer", "x-talos-file-transfer",
        "origin", "access-control-request-headers", "access-control-request-method"]) {
        const value = request.headers.get(name);
        if (value !== null) headers.set(name, value);
      }
      const options = { method: request.method, headers, body: request.body, signal: request.signal,
        redirect: "error" as const, duplex: "half" as const, dispatcher };
      return await fetch(target, options);
    } catch (error) {
      if (request.signal.aborted) throw error;
      console.error("Desktop control-plane request failed", error);
      const apiError = error instanceof TalosApiError ? error.apiError : {
        code: AppProtocol.ErrorCode.ServiceUnavailable, message: "The Talos Server is unavailable.", localization: null,
      };
      return Response.json({ status: "error", error: apiError }, { status: 503 });
    }
  };
}
