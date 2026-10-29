import { normalizeWorkspaceWebpageUrl } from "@/shared/store/workspace-window";

/** Complete human address input before applying the existing URL validation. */
export function parseBrowserAddress(value: string): string | null {
  const address = value.trim();
  return normalizeWorkspaceWebpageUrl(/^[a-z][a-z\d+.-]*:\/\//i.test(address) ? address : `http://${address}`);
}
