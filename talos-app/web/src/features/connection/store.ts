import type { AccountType } from "./account-type";
import type { AuthStatus } from "@/shared/types/account";

type UserRole = "user" | "admin";

export interface ConnectionState {
  workerRuntimeState?: import("@talos/sdk-app-server/desktop").TalosRuntimeState;
  controlPlaneEndpoint?: string;
  authError?: import("@talos/sdk-core").AppProtocol.ApiError;
  authToken: string;
  authStatus: AuthStatus;
  authRole: UserRole;
  accountType: AccountType;
  authUsername: string;
  userId: string;
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
      accountType: "Free",
      authUsername: "Anonymous",
      userId: "anonymous",
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
