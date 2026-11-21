import { useAppStore } from "@/shared/store/app-store";
import { canUseTalo } from "@/shared/types/account";

export function useCanUseTalo() {
  return useAppStore(({ connection }) => canUseTalo(connection));
}
