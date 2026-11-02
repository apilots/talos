import { TalosApiError, TalosHttpError, parseTalosJson } from "./adapters";
import { ErrorCode } from "./generated/app-protocol";
import type * as API from "./generated/app-protocol";
export interface TrajectoryTransport {
    capabilities(): Promise<API.TrajectoryCapabilitiesResponse>;
    readTurn(request: API.TrajectoryReadTurnRequest, signal?: AbortSignal): Promise<API.TrajectoryReadTurnResponse>;
    readRequest(request: API.TrajectoryReadModelRequestRequest, signal?: AbortSignal): Promise<API.TrajectoryReadModelRequestResponse>;
    readTool(request: API.TrajectoryReadToolRequest, signal?: AbortSignal): Promise<API.TrajectoryReadToolResponse>;
    readPayload(request: API.TrajectoryReadPayloadRequest, signal?: AbortSignal): Promise<API.TrajectoryReadPayloadResponse>;
}
function invalidPayload(): TalosApiError {
    return new TalosApiError({ code: ErrorCode.InternalError, message: "The recorded trajectory payload is invalid or incomplete.", localization: null });
}
function object(value: unknown): value is Record<string, unknown> { return value !== null && typeof value === "object" && !Array.isArray(value); }
/** Shared bounded assembly of immutable historical payloads. */
export function createTrajectoryClient(transport: TrajectoryTransport) {
    return { ...transport,
        async capabilities(): Promise<API.TrajectoryCapabilitiesResponse> {
            try {
                return await transport.capabilities();
            }
            catch (error) {
                // Only an absent capability route/method means an old unsupported host.
                if ((error instanceof TalosHttpError && error.status === 404) || (error instanceof TalosApiError && error.code === ErrorCode.ResourceNotFound) || (object(error) && error.code === -32601))
                    return { supported: false };
                throw error;
            }
        },
        async payload(identity: API.AgentTurnRef, ref: API.PayloadRef | null, signal?: AbortSignal): Promise<API.TrajectoryPayload | null> {
            if (ref === null)
                return null;
            let cursor: string | null = null, body = "", bytes = 0;
            const seen = new Set<string>();
            do {
                signal?.throwIfAborted();
                const page = await transport.readPayload({ identity, payloadId: ref.payloadId, cursor, limitBytes: 256 * 1024 }, signal);
                if (page.payloadId !== ref.payloadId || page.encoding !== "utf8Json" || typeof page.data !== "string")
                    throw invalidPayload();
                const size = new TextEncoder().encode(page.data).byteLength;
                bytes += size;
                if (size > 256 * 1024 || bytes > 4 * 1024 * 1024 || (page.nextCursor !== null && (size === 0 || seen.has(page.nextCursor))))
                    throw invalidPayload();
                body += page.data;
                cursor = page.nextCursor;
                if (cursor !== null)
                    seen.add(cursor);
            } while (cursor !== null);
            // The shared Rust history owner validates payload shape before paging.
            return parseTalosJson(body) as API.TrajectoryPayload;
        },
    };
}
