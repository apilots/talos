import type { RuntimeMode } from "@/shared/types/ui";

type UserRole = "user" | "admin";
type AuthStatus =
  | "unknown"
  | "loading"
  | "authenticated"
  | "unauthenticated"
  | "logged_out"
  | "open";

export interface ConnectionState {
  authToken: string;
  authStatus: AuthStatus;
  authRole: UserRole;
  authUsername: string;
  userId: string;
  runtimeMode: RuntimeMode;
}

export interface ConnectionSlice {
  connection: ConnectionState;
  setConnection: (patch: Partial<ConnectionState>) => void;
}

export type ConnectionSliceSetter = (
  updater: (state: ConnectionSlice) => Partial<ConnectionSlice>,
) => void;

export function createConnectionSlice(set: ConnectionSliceSetter): ConnectionSlice {
  return {
    connection: {
      authToken: "",
      authStatus: "unknown",
      authRole: "user",
      authUsername: "Anonymous",
      userId: "anonymous",
      runtimeMode: "local",
    },
    setConnection: (patch) =>
      set((state) => {
        const nextConnection = {
          ...state.connection,
          ...patch,
          runtimeMode: normalizeRuntimeMode(patch.runtimeMode ?? state.connection.runtimeMode),
        };
        const changed = Object.keys(patch).some((key) => {
          const typedKey = key as keyof ConnectionState;
          return state.connection[typedKey] !== nextConnection[typedKey];
        });

        if (!changed) {
          return {
            connection: state.connection,
          };
        }

        return {
          connection: nextConnection,
        };
      }),
  };
}

function normalizeRuntimeMode(value: RuntimeMode): RuntimeMode {
  return value === "local" ? "local" : "local";
}

export function resolveServerBaseUrl(): string {
  const configured = import.meta.env.VITE_API_BASE_URL?.trim();
  if (configured) {
    return configured;
  }
  if (typeof window !== "undefined" && window.location.origin !== "null") {
    return window.location.origin;
  }
  return "http://localhost:8080";
}
