import type { ApprovalMode, RuntimeMode } from "@/shared/types/ui";

const defaultServerUrl = import.meta.env.VITE_API_BASE_URL ?? "http://localhost:8080";
type UserRole = "user" | "admin";
type AuthStatus = "unknown" | "loading" | "authenticated" | "unauthenticated";

export interface ConnectionState {
  serverUrl: string;
  authToken: string;
  authStatus: AuthStatus;
  authRole: UserRole;
  authUsername: string;
  userId: string;
  runtimeMode: RuntimeMode;
  defaultApprovalMode: ApprovalMode;
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
      authStatus: "unauthenticated",
      authRole: "user",
      authUsername: "web-user",
      userId: "web-user",
      runtimeMode: "cloud",
      defaultApprovalMode: "needs_approval",
    },
    setConnection: (patch) =>
      set((state) => {
        const nextConnection = {
          ...state.connection,
          ...patch,
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
