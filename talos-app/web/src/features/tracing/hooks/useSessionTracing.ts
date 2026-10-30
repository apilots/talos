import { useEffect, useCallback } from "react";
import { AppProtocol as API, TalosApiError } from "@talos/sdk-core";
import { useQueries, useQuery, useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { useConnectionKey } from "@/features/chat";
import { getTalosClient } from "@/shared/api/client";
import type { TraceSelection } from "../model/selectors";
export type TraceTab =
  | "summary"
  | "prompt"
  | "tools"
  | "response"
  | "raw"
  | "timing"
  | "arguments"
  | "execution"
  | "modelResult";
/** One query lifecycle owner; views never create subscriptions or replay events. */
export function useSessionTracing(
  sessionId: string,
  turnId: string | undefined,
  visible: boolean,
  selection: TraceSelection | null,
  tab: TraceTab,
  consumerId: string,
  rawField: string,
  turnIds: string[],
) {
  const connection = useConnectionKey();
  const scope = ["trajectory", connection, sessionId] as const;
  const api = getTalosClient().trajectory;
  const identity = { sessionId, turnId: turnId ?? "" };
  const capability = useQuery({
    queryKey: [...scope, "capabilities"],
    queryFn: () => api.capabilities(),
    enabled: visible,
    retry: false,
    staleTime: 60000,
  });
  const enabled = visible && capability.data?.supported === true && Boolean(turnId);
  const combineTimelines = useCallback((results: UseQueryResult<{ pages: API.TrajectoryReadTurnResponse[] }>[]) =>
    results.map((result, index) => ({
      turnId: turnIds[index],
      data: result.data,
      isPending: result.isPending,
      error: result.error,
      clocks: result.data?.pages[0]?.clockSegments ?? [],
      entries: result.data?.pages.flatMap((page) => page.data ?? []) ?? [],
      loading: result.isPending,
    })), [turnIds]);
  const timelines = useQueries({
    combine: combineTimelines,
    queries: turnIds.map((id) => ({
      queryKey: [...scope, "turn", id],
      queryFn: async ({ signal }: { signal: AbortSignal }) => {
        const pages: API.TrajectoryReadTurnResponse[] = [];
        let cursor: string | null = null;
        do {
          signal.throwIfAborted();
          const page = await api.readTurn({
            identity: { sessionId, turnId: id }, cursor, limit: 32,
          }, signal);
          pages.push(page);
          cursor = page.nextCursor;
        } while (cursor != null);
        return { pages };
      },
      enabled: visible && capability.data?.supported === true,
      retry: false,
      staleTime: Infinity,
    })),
  });
  const turn = timelines[turnIds.indexOf(turnId ?? "")] ?? {
    data: undefined, error: null, isPending: false,
  };
  const cache = useQueryClient();
  useEffect(() => {
    if (!visible)
      void cache.cancelQueries({
        queryKey: ["trajectory", connection, sessionId],
      });
  }, [cache, connection, sessionId, visible]);
  const tool = useQuery({
    queryKey: [
      ...scope,
      "tool",
      turnId,
      selection?.kind === "tool" ? selection.callId : null,
    ],
    queryFn: async ({ signal }) => {
      try {
        return await api.readTool(
          { identity, callId: selection?.kind === "tool" ? selection.callId : "" },
          signal,
        );
      } catch (error) {
        const knownTool = turn.data?.pages.some((page) => page.data?.some(
          (entry) => entry.type === "tool" &&
            selection?.kind === "tool" && entry.tool.callId === selection.callId,
        ));
        // Canonical History can contain tools without optional capture metadata.
        if (!knownTool && error instanceof TalosApiError && error.code === API.ErrorCode.ResourceNotFound)
          return null;
        throw error;
      }
    },
    enabled:
      enabled &&
      selection?.kind === "tool" &&
      tab !== "execution" &&
      turn.data?.pages[0]?.data != null,
    retry: false,
  });
  const modelRequestId =
    selection?.kind === "request"
      ? selection.modelRequestId
      : selection?.kind === "tool"
        ? tab === "arguments"
          ? (tool.data?.tool.sourceModelRequestId ?? undefined)
          : tab === "modelResult" && consumerId !== "prepared"
            ? consumerId
            : undefined
        : undefined;
  const request = useQuery({
    queryKey: [...scope, "request", turnId, modelRequestId],
    queryFn: ({ signal }) =>
      api.readRequest({ identity, modelRequestId: modelRequestId! }, signal),
    enabled: enabled && Boolean(modelRequestId),
    retry: false,
  });
  const callRecord =
    request.data?.request.callRecords.find((a) =>
      selection?.kind === "request"
        ? a.callRecordId === selection.callRecordId
        : selection?.kind === "tool" &&
          a.callRecordId === tool.data?.tool.sourceCallRecordId,
    ) ?? request.data?.request.callRecords.at(-1);
  const field = tab === "raw" ? rawField : tab;
  const bodyRef =
    selection?.kind === "tool"
      ? tab === "arguments"
        ? request.data?.outputs.find((o) => o.callRecordId === callRecord?.callRecordId)
            ?.response
        : tab === "modelResult"
          ? consumerId === "prepared"
            ? tool.data?.preparedResult
            : request.data?.prompt
          : undefined
      : field === "response"
        ? request.data?.outputs.find((o) => o.callRecordId === callRecord?.callRecordId)
            ?.response
        : field === "tools"
          ? request.data?.tools
          : field === "parameters"
            ? request.data?.parameters
            : request.data?.prompt;
  const refs = [
    bodyRef,
    selection?.kind === "request" ? request.data?.parameters : undefined,
  ];
  const bodyWanted =
    selection?.kind === "request"
      ? ["prompt", "tools", "response", "raw"].includes(tab)
      : selection?.kind === "tool" &&
        ["arguments", "modelResult"].includes(tab);
  const uniqueRefs = [
    ...new Map(
      refs
        .filter((ref): ref is API.PayloadRef => ref != null)
        .map((ref) => [ref.payloadId, ref]),
    ).values(),
  ];
  const bodies = useQueries({
    queries: uniqueRefs.map((ref) => ({
      queryKey: [...scope, "payload", turnId, ref.payloadId],
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        api.payload(identity, ref, signal),
      enabled:
        enabled &&
        (bodyWanted || ref.payloadId === request.data?.parameters?.payloadId),
      retry: false,
      staleTime: Infinity,
      gcTime: 60000,
    })),
  });
  const loadedRequests = cache
    .getQueriesData<API.TrajectoryReadModelRequestResponse>({
      queryKey: [...scope, "request", turnId],
    })
    .flatMap(([, value]) => (value ? [value] : []));
  const loadedTools = cache
    .getQueriesData<API.TrajectoryReadToolResponse>({
      queryKey: [...scope, "tool", turnId],
    })
    .flatMap(([, value]) => (value ? [value] : []));
  const loadedPayloads = new Map(
    cache
      .getQueriesData<API.TrajectoryPayload>({
        queryKey: [...scope, "payload", turnId],
      })
      .flatMap(([key, value]) =>
        value && typeof key.at(-1) === "string"
          ? [[key.at(-1) as string, value] as const]
          : [],
      ),
  );
  const empty = { data: undefined, isFetching: false, isError: false };
  const body =
    bodies[
      uniqueRefs.findIndex((ref) => ref.payloadId === bodyRef?.payloadId)
    ] ?? empty;
  const parameters =
    bodies[
      uniqueRefs.findIndex(
        (ref) => ref.payloadId === request.data?.parameters?.payloadId,
      )
    ] ?? empty;
  return {
    timelines,
    loadedRequests,
    loadedTools,
    loadedPayloads,
    retryRead: () => cache.invalidateQueries({ queryKey: scope }),
    capability,
    turn,
    request,
    tool,
    callRecord,
    bodyRef,
    body,
    parameters,
    entries: "entries" in turn ? turn.entries : [],
  };
}
