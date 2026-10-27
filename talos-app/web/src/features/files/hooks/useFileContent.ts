import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getTalosClient } from "@/shared/api/client";

export function useFileContent({ scope, fileId, instanceKey, enabled }: { scope: string; fileId: string; instanceKey: string; enabled: boolean }) {
  return useQuery({
    queryKey: ["talos", scope, "file", fileId, "content", instanceKey],
    queryFn: () => getTalosClient().files.content({ fileId }),
    enabled, refetchOnWindowFocus: false, retry: false,
  });
}

export function useFileObjectUrl(data: Uint8Array | undefined, mimeType: string | undefined) {
  const [source, setSource] = useState<{ data: Uint8Array; url: string }>();
  useEffect(() => {
    if (!data || !mimeType) { setSource(undefined); return; }
    const url = URL.createObjectURL(new Blob([new Uint8Array(data).buffer], { type: mimeType }));
    setSource({ data, url });
    return () => URL.revokeObjectURL(url);
  }, [data, mimeType]);
  return source?.data === data ? source?.url : undefined;
}

export function downloadFile(data: Uint8Array, filename: string, mimeType: string) {
  const url = URL.createObjectURL(new Blob([new Uint8Array(data).buffer], { type: mimeType }));
  const link = document.createElement("a");
  link.href = url; link.download = filename; document.body.append(link); link.click(); link.remove();
  // Keep the Blob alive while the browser consumes the user-initiated download.
  window.setTimeout(() => URL.revokeObjectURL(url), 30_000);
}
