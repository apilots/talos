import type { AppProtocol as API } from "@talos/sdk-core";
export const identity = { sessionId: "s", turnId: "t" };
const usage = {
  input_tokens: 400,
  output_tokens: 20,
  total_tokens: 420,
  cache_creation_input_tokens: null,
  cache_read_input_tokens: null,
};
export const history: API.SessionHistoryResponse = {
  nextCursor: null,
  lastStableEventId: null,
  data: [
    {
      identity,
      completedAt: 2,
      completedEventSeq: 3,
      notices: [],
      result: {
        status: "completed",
        result: {
          completion: { stopReason: "endTurn" },
          metrics: {
            modelRounds: 2,
            toolCalls: 1,
            toolSuccesses: 1,
            toolErrors: 0,
            usage,
            context: null,
            durationMs: 1500,
          },
          messages: [
            {
              messageId: "m",
              turnId: "t",
              role: "agent",
              status: "completed",
              createdAt: 1,
              completedAt: 2,
              blocks: [
                {
                  blockId: "b",
                  status: "completed",
                  startedAt: 1,
                  completedAt: 2,
                  content: {
                    type: "assistantText",
                    data: { phase: "commentary", text: "阶段反馈保持原文" },
                  },
                },
              ],
            },
          ],
        },
      },
    },
  ],
};
export const request: API.ModelRequestTrace = {
  modelRequestId: "r",
  ordinal: 1,
  purpose: "working",
  modelId: "model",
  connectionId: "configured",
  callRecords: [
    {
      callRecordId: "a",
      ordinal: 1,
      observedState: "completed",
      metrics: {
        segmentId: "seg", startedAtMs: 100000, finishedAtMs: 101000,
        executionDurationMs: 1000, firstOutputAtMs: 100100, lastOutputAtMs: 100900,
        ttftMs: 100, generationMs: 800, usage,
      },
      finishReason: "stop",
      error: null,
    },
  ],
};
export const ref = { payloadId: "p", complete: true } as const;

export const prompt: API.TrajectoryPayload = {
  kind: "prompt",
  messages: [
    {
      index: 0,
      role: "system",
      content: "exact prompt",
      reasoning: null,
      name: null,
      toolCallId: null,
      toolCalls: null,
      attachments: [],
      cacheControl: { type: "none" },
      inputOrigin: null,
    },
  ],
};
