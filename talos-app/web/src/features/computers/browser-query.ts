import {useQuery} from "@tanstack/react-query";
import {getTalosClient} from "@/shared/api/client";
import {useProductScope} from "./hooks";

/** One account/Computer snapshot shared by Settings and all viewer entries. */
export function useBrowser(nodeId: string | undefined, enabled = true) {
  const scope = useProductScope();
  return useQuery({
    queryKey: ["talos", scope, "browser", nodeId],
    queryFn: () => getTalosClient(nodeId).browser.operate({operation: {kind: "read"}}),
    enabled: Boolean(nodeId) && enabled, refetchInterval: 2000, retry: false,
  });
}
