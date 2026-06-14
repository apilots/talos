import type { RuntimeMode } from "@/shared/types/ui";

const defaultServerUrl = import.meta.env.VITE_API_BASE_URL ?? "http://localhost:8080";
type UserRole = "user" | "admin";
type AuthStatus =
  | "unknown"
  | "loading"
  | "authenticated"
  | "unauthenticated"
  | "logged_out"
  | "open";

export interface ConnectionState {
  serverUrl: string;
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
      serverUrl: defaultServerUrl,
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
