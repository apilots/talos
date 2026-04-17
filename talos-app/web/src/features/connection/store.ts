import type { ApprovalMode, RuntimeMode } from "@/shared/types/ui";

const defaultServerUrl = import.meta.env.VITE_API_BASE_URL ?? "http://localhost:8080";

export interface ConnectionState {
  serverUrl: string;
  authToken: string;
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
      userId: "web-user",
      runtimeMode: "cloud",
      defaultApprovalMode: "needs_approval",
    },
    setConnection: (patch) =>
      set((state) => ({
        connection: {
          ...state.connection,
          ...patch,
        },
      })),
  };
}
