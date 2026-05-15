// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import type { TalosSession } from "@talos/sdk-browser";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import type { AdminPageState } from "../hooks/useAdminPage";
import { AdminSessionsSection } from "./AdminSessionsSection";

const navigateMock = vi.fn();
const deleteAdminSessionMock = vi.fn();

vi.mock("react-router-dom", async () => {
  const actual = await vi.importActual<typeof import("react-router-dom")>("react-router-dom");
  return {
    ...actual,
    useNavigate: () => navigateMock,
  };
});

vi.mock("@/features/chat", () => ({
  chatKeys: {
    sessions: (connectionKey: string) => ["talos", connectionKey, "sessions"],
    session: (connectionKey: string, sessionId: string) => ["talos", connectionKey, "session", sessionId],
    liveRecovery: (connectionKey: string, sessionId: string) =>
      ["talos", connectionKey, "live-recovery", sessionId],
  },
  deleteAdminSession: (sessionId: string) => deleteAdminSessionMock(sessionId),
  useConnectionKey: () => "test-connection",
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
      mutations: {
        retry: false,
      },
    },
  });

  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function buildProps(): ComponentProps<typeof AdminSessionsSection> {
  const session: TalosSession = {
    title: "Wild Session",
    user_id: "owner-1",
    context: {
      session_id: "sess-1",
      user_id: "source-1",
      source_type: "http",
      provider: "sdk",
      message_id: "msg-1",
      trace_id: "trace-1",
      channel_id: "web",
    },
    state: {
      last_activity: Date.now() - 1_000,
      created_at: Date.now() - 10_000,
      message_count: 0,
      usage: {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
      },
    },
    conf: {},
    config: {
      agent: {
        model: "glm-5",
      },
    },
  };

  return {
    sessions: {
      data: {
        data: [session],
        pagination: {
          total: 42,
          limit: 20,
          offset: 0,
        },
      },
      isFetching: false,
    } as AdminPageState["sessions"],
    sessionSearch: "",
    sessionPagination: {
      page: 1,
      pageSize: 20,
      total: 42,
      totalPages: 3,
      offset: 0,
      from: 1,
      to: 1,
      hasPrevious: false,
      hasNext: true,
    },
    setSessionPage: vi.fn(),
    setSessionSearch: vi.fn(),
    sessionRows: [
      {
        session,
        sessionId: "sess-1",
        ownerUserId: "owner-1",
        ownerLabel: "xujiang",
        sourceUserId: "source-1",
        label: "Wild Session",
        model: "glm-5",
        status: "done" as const,
      },
    ],
  };
}

describe("AdminSessionsSection", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(() => {
    cleanup();
  });

  it("deletes a session from the admin actions group", async () => {
    vi.stubGlobal("confirm", vi.fn(() => true));
    deleteAdminSessionMock.mockResolvedValue(undefined);

    render(<AdminSessionsSection {...buildProps()} />, { wrapper: createWrapper() });

    fireEvent.click(screen.getAllByRole("button", { name: "删除" })[0]!);

    await waitFor(() => {
      expect(deleteAdminSessionMock).toHaveBeenCalledWith("sess-1");
    });
  });

  it("hides the row immediately after delete is confirmed", async () => {
    vi.stubGlobal("confirm", vi.fn(() => true));
    let resolveDelete: (() => void) | undefined;
    deleteAdminSessionMock.mockImplementation(
      () => new Promise<void>((resolve) => {
        resolveDelete = resolve;
      }),
    );

    render(<AdminSessionsSection {...buildProps()} />, { wrapper: createWrapper() });

    fireEvent.click(screen.getAllByRole("button", { name: "删除" })[0]!);

    await waitFor(() => {
      expect(screen.queryByText("Wild Session")).toBeNull();
    });

    resolveDelete?.();
  });

  it("shows a friendly action error when delete fails", async () => {
    vi.stubGlobal("confirm", vi.fn(() => true));
    deleteAdminSessionMock.mockRejectedValue(new Error("delete failed"));

    render(<AdminSessionsSection {...buildProps()} />, { wrapper: createWrapper() });

    fireEvent.click(screen.getAllByRole("button", { name: "删除" })[0]!);

    expect(await screen.findByText("操作失败：delete failed")).toBeInTheDocument();
  });

  it("renders pagination controls and advances to the next page", async () => {
    const props = buildProps();

    render(<AdminSessionsSection {...props} />, { wrapper: createWrapper() });

    expect(screen.getByText("第 1-1 条，共 42 条会话")).toBeInTheDocument();
    expect(screen.getByText("第 1 / 3 页")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "下一页" }));

    expect(props.setSessionPage).toHaveBeenCalledWith(2);
  });
});
