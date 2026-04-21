import { getConnectionSettings } from "@/shared/store/app-store";

export interface UserTokenMeta {
  token_id: string;
  name: string;
  prefix: string;
  created_at: number;
  last_used_at: number | null;
  expires_at: number | null;
}

export interface CreateUserTokenResult {
  token_id: string;
  raw_token: string;
  prefix: string;
}

interface TokenListResponse {
  data: UserTokenMeta[];
}

interface DeleteTokenResponse {
  success: boolean;
}

async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
  const { serverUrl, authToken } = getConnectionSettings();
  const response = await fetch(new URL(path, serverUrl).toString(), {
    ...init,
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      ...(authToken ? { Authorization: `Bearer ${authToken}` } : {}),
      ...init?.headers,
    },
  });

  if (!response.ok) {
    const body = await response.text();
    let message = body || `${response.status} ${response.statusText}`;
    try {
      const parsed = JSON.parse(body) as { message?: string };
      if (parsed.message) {
        message = parsed.message;
      }
    } catch {
      // Keep raw response text when the body is not JSON.
    }
    throw new Error(message);
  }

  return response.json() as Promise<T>;
}

export async function listUserTokens(): Promise<UserTokenMeta[]> {
  const response = await requestJson<TokenListResponse>("/v1/user/tokens");
  return response.data;
}

export async function createUserToken(name: string): Promise<CreateUserTokenResult> {
  return requestJson<CreateUserTokenResult>("/v1/user/tokens", {
    method: "POST",
    body: JSON.stringify({ name }),
  });
}

export async function revokeUserToken(tokenId: string): Promise<void> {
  const response = await requestJson<DeleteTokenResponse>(`/v1/user/tokens/${tokenId}`, {
    method: "DELETE",
  });
  if (!response.success) {
    throw new Error("Failed to revoke token.");
  }
}
