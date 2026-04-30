import type {
  AdminCreateUserRequest,
  AdminPatchUserRequest,
  AuthResponse,
  LoginRequest,
  RegisterRequest,
  UpdateCurrentUserRequest,
  UserDto,
} from "@talos/sdk-browser";

import { getTalosClient } from "@/shared/api/client";
import { getConnectionSettings } from "@/shared/store/app-store";

export interface HealthResponse {
  status: string;
  service: string;
  timestamp: string;
  auth_enabled: boolean;
}

export async function registerUser(request: RegisterRequest): Promise<AuthResponse> {
  return getTalosClient().auth.register(request);
}

export async function loginUser(request: LoginRequest): Promise<AuthResponse> {
  return getTalosClient().auth.login(request);
}

export async function logoutUser(): Promise<void> {
  return getTalosClient().auth.logout();
}

export async function getCurrentUser(): Promise<UserDto> {
  return getTalosClient().user.current();
}

export async function getGatewayHealth(): Promise<HealthResponse> {
  const { serverUrl } = getConnectionSettings();
  const response = await fetch(new URL("/health", serverUrl).toString(), {
    headers: {
      Accept: "application/json",
    },
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<HealthResponse>;
}

export async function updateCurrentUser(request: UpdateCurrentUserRequest): Promise<UserDto> {
  return getTalosClient().user.update(request);
}

export async function createUser(request: AdminCreateUserRequest): Promise<UserDto> {
  return getTalosClient().admin.users.create(request);
}

export async function listUsers(): Promise<UserDto[]> {
  return getTalosClient().admin.users.list();
}

export async function patchUser(userId: string, request: AdminPatchUserRequest): Promise<UserDto> {
  return getTalosClient().admin.users.patch(userId, request);
}

export async function deleteUser(userId: string): Promise<void> {
  return getTalosClient().admin.users.delete(userId);
}
