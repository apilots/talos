import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { createUserToken, listUserTokens, revokeUserToken } from "../api";
import { useConnectionKey } from "@/features/chat";

export function useUserTokens() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const queryKey = ["talos", connectionKey, "user-tokens"] as const;

  const tokensQuery = useQuery({
    queryKey,
    queryFn: listUserTokens,
  });

  const createMutation = useMutation({
    mutationFn: (name: string) => createUserToken(name),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey });
    },
  });

  const revokeMutation = useMutation({
    mutationFn: (tokenId: string) => revokeUserToken(tokenId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey });
    },
  });

  return {
    tokensQuery,
    createToken: (name: string) => createMutation.mutateAsync(name),
    revokeToken: (tokenId: string) => revokeMutation.mutateAsync(tokenId),
    createMutation,
    revokeMutation,
  };
}
