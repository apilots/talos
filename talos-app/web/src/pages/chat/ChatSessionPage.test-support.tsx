import { vi } from "vitest";
import type { ComponentProps } from "react";

const mocks = {
  sendText: vi.fn(),
  draft: "",
  realComposer: false,
  sendTurn: vi.fn(),
  setDraft: vi.fn(),
  refetchProjects: vi.fn(),
  mutateModel: vi.fn(),
  mutatePermissionMode: vi.fn(),
  submitUserAction: vi.fn(),
  compactSession: vi.fn(),
  clearSession: vi.fn(),
  session: undefined as unknown,
  sessionHistoryError: undefined as Error | undefined,
  sessionHistoryIsSuccess: true,
  sessionQueryEnabled: undefined as boolean | undefined,
  isStreaming: false,
  composerProps: undefined as unknown,
};

export function getChatSessionPageMocks() {
  return mocks;
}

export async function createChatFeatureMock(
  importOriginal: () => Promise<typeof import("@/features/chat")>,
) {
  const actual = await importOriginal();

  return {
    ...actual,
    ChatComposer: (props: ComponentProps<typeof actual.ChatComposer>) => {
      mocks.composerProps = props;
      if (mocks.realComposer) return <actual.ChatComposer {...props} />;
      return (
        <div data-disabled={props.disabled ? "true" : "false"} data-testid="chat-composer">
          {props.commandResponse ? <div>{props.commandResponse.kind}</div> : null}
          <button onClick={() => props.onModelChange?.("model-2")} type="button">
            Model
          </button>
          <button onClick={() => props.onProjectChange("project-1")} type="button">
            Project
          </button>
          <button onClick={() => props.onReasoningLevelChange?.("high")} type="button">
            Reasoning
          </button>
          <button onClick={() => void Promise.resolve(props.onCommand?.("compact")).catch(() => undefined)} type="button">
            Compact
          </button>
          <button onClick={() => void props.onSend([])} type="button">
            Send
          </button>
        </div>
      );
    },
    useChatSession: () => ({
      blocks: [],
      draft: mocks.draft,
      setDraft: mocks.setDraft,
      sendTurn: mocks.sendTurn,
      sendText: mocks.sendText,
      isStreaming: mocks.isStreaming,
      isCancelling: false,
      canRetryResponse: false,
      retryResponse: vi.fn(),
      retryStream: vi.fn(),
      showReconnectControls: false,
      stopResponse: vi.fn(),
      streamError: undefined,
      messageQueue: {
        queue: undefined,
        error: undefined,
        add: vi.fn(),
        update: vi.fn(),
        delete: vi.fn(),
        reorder: vi.fn(),
        start: vi.fn(),
        steer: vi.fn(),
      },
      sessionHistoryQuery: {
        error: mocks.sessionHistoryError,
        isError: Boolean(mocks.sessionHistoryError),
        isLoading: false,
        isSuccess: mocks.sessionHistoryIsSuccess,
      },
    }),
    useModelsQuery: () => ({
      data: {
        defaultModelSelection: {
          modelRef: { provider: "mock", id: "model-1" },
          reasoningEffort: "medium",
        },
        models: [
          {
            id: "model-1",
            name: "Model 1",
            provider: "mock",
            reasoning: true,
            reasoning_effort: "medium",
            selection_capabilities: { reasoning: "effort", reasoning_efforts: ["light", "medium", "high", "xhigh"], fast: true, default_fast: false, default_reasoning_effort: "medium" },
          },
        ],
      },
    }),
    useProjectsView: () => ({
      refetch: mocks.refetchProjects,
      activeProjectId: "__unbound_project__",
      projects: [
        {
          id: "project-1",
          name: "Project 1",
          directory: "/tmp/project-1",
        },
      ],
      setActiveProjectId: vi.fn(),
    }),
    useSessionQuery: (_sessionId: string, enabled?: boolean) => {
      mocks.sessionQueryEnabled = enabled;
      return { data: mocks.session };
    },
    useUpdateSessionModelMutation: () => ({ mutate: mocks.mutateModel }),
    useUpdateSessionPermissionModeMutation: () => ({
      isPending: false,
      mutate: mocks.mutatePermissionMode,
    }),
    useSubmitUserActionMutation: () => ({
      error: undefined,
      isPending: false,
      mutate: mocks.submitUserAction,
    }),
    useSessionActions: () => ({
      compact: { isPending: false, mutateAsync: mocks.compactSession },
      clear: { isPending: false, mutateAsync: mocks.clearSession },
    }),
  };
}

export function createAppStoreMock() {
  return {
    useAppStore: (selector: (state: Record<string, unknown>) => unknown) =>
      selector({
        connection: {}, selectComputer: vi.fn(),
      }),
  };
}

export function resetChatSessionPageMocks() {
  mocks.session = {
    context: { session_id: "sess-1" },
    title: "Session 1",
    projectId: null,
    config: { llm: { model: "model-1" } },
    state: { message_count: 0 },
    modelSelection: {
      modelRef: { provider: "mock", id: "model-1" },
      reasoningEffort: null,
    },
  };
  mocks.realComposer = false;
  mocks.draft = "";
  mocks.sendTurn.mockReset();
  mocks.sendTurn.mockResolvedValue(undefined);
  mocks.sessionHistoryError = undefined;
  mocks.sessionHistoryIsSuccess = true;
  mocks.isStreaming = false;
  mocks.composerProps = undefined;
  mocks.sessionQueryEnabled = undefined;
  mocks.setDraft.mockReset();
}
