import { useAppStore } from "@/shared/store/app-store";

export function useCanUseTalo() {
  return useAppStore(({ connection }) =>
    connection.authStatus === "authenticated" && (connection.authRole === "admin" || connection.accountType === "Max"));
}
