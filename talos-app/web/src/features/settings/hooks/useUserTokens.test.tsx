// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useUserTokens } from "./useUserTokens";

const listUserTokens = vi.fn();
const createUserToken = vi.fn();
const revokeUserToken = vi.fn();

vi.mock("../api", () => ({
  listUserTokens: () => listUserTokens(),
  createUserToken: (name: string) => createUserToken(name),
  revokeUserToken: (tokenId: string) => revokeUserToken(tokenId),
}));

vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "settings-connection",
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

describe("useUserTokens", () => {
  let tokens = [
    {
      token_id: "tok-1",
      name: "CLI",
      prefix: "tls_alpha123",
      created_at: 1_776_393_847_462,
      last_used_at: null,
      expires_at: null,
    },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    tokens = [
      {
        token_id: "tok-1",
        name: "CLI",
        prefix: "tls_alpha123",
        created_at: 1_776_393_847_462,
        last_used_at: null,
        expires_at: null,
      },
    ];

    listUserTokens.mockImplementation(async () => tokens);
    createUserToken.mockImplementation(async (name: string) => {
      const created = {
        token_id: "tok-2",
        raw_token: "tls_secret_token",
        prefix: "tls_beta456",
      };
      tokens = [
        ...tokens,
        {
          token_id: created.token_id,
          name,
          prefix: created.prefix,
          created_at: 1_776_393_947_462,
          last_used_at: null,
          expires_at: null,
        },
      ];
      return created;
    });
    revokeUserToken.mockImplementation(async (tokenId: string) => {
      tokens = tokens.filter((token) => token.token_id !== tokenId);
    });
  });

  it("lists tokens and refreshes after creation", async () => {
    const { result } = renderHook(() => useUserTokens(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.tokensQuery.data).toHaveLength(1);
    });

    await act(async () => {
      const created = await result.current.createToken("Automation");
      expect(created.raw_token).toBe("tls_secret_token");
    });

    await waitFor(() => {
      expect(result.current.tokensQuery.data).toHaveLength(2);
      expect(result.current.tokensQuery.data?.[1]?.name).toBe("Automation");
    });
  });

  it("refreshes the token list after revoke", async () => {
    const { result } = renderHook(() => useUserTokens(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.tokensQuery.data).toHaveLength(1);
    });

    await act(async () => {
      await result.current.revokeToken("tok-1");
    });

    await waitFor(() => {
      expect(result.current.tokensQuery.data).toHaveLength(0);
    });
  });
});
