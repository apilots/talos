// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkspaceDocsPage } from "./useWorkspaceDocsPage";

const getWorkspaceDoc = vi.fn();
const putWorkspaceDoc = vi.fn();

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({
    workspaceDocs: {
      get: getWorkspaceDoc,
      put: putWorkspaceDoc,
    },
  }),
}));

vi.mock("@/features/chat", () => ({
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
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("useWorkspaceDocsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    getWorkspaceDoc.mockImplementation(async (kind: string) => ({
      kind,
      path: `${kind.toUpperCase()}.md`,
      exists: true,
      content: `# ${kind}`,
      updated_at: 1776393847462,
      version_token: `${kind}-token`,
    }));

    putWorkspaceDoc.mockImplementation(async (kind: string, request: { content: string }) => ({
      kind,
      path: `${kind.toUpperCase()}.md`,
      exists: true,
      content: request.content,
      updated_at: 1776393847462,
      version_token: `${kind}-next-token`,
    }));
  });

  it("loads the selected workspace document", async () => {
    const { result } = renderHook(() => useWorkspaceDocsPage(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.selectedDetail.kind).toBe("memory");
      expect(result.current.docDraft).toBe("# memory");
    });
  });

  it("saves the current workspace document with version matching", async () => {
    const { result } = renderHook(() => useWorkspaceDocsPage(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.selectedDetail.version_token).toBe("memory-token");
    });

    act(() => {
      result.current.setDocDraft("# updated");
    });

    await act(async () => {
      await result.current.saveCurrent();
    });

    expect(putWorkspaceDoc).toHaveBeenCalledWith("memory", {
      content: "# updated",
      base_version: {
        mode: "match",
        version_token: "memory-token",
      },
    });
  });
});
