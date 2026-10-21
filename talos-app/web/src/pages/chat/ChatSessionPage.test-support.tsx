import { vi } from "vitest";

const mocks = {
  sendText: vi.fn(),
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
    ChatComposer: (props: {
      commandResponse?: { kind: string; state?: string };
      disabled?: boolean;
      onModelChange?: (model: string) => void;
      onCommand?: (command: "compact" | "clear" | "status" | "model") => Promise<void> | void;
      onProjectChange: (projectId: string) => void;
      onReasoningLevelChange?: (level: "light" | "medium" | "high" | "extra_high") => void;
      onSend: (attachments: []) => Promise<void>;
    }) => {
      mocks.composerProps = props;
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
      draft: "",
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
