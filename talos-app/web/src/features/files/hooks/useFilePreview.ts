import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import type { FilePreviewDescriptor, FilePreviewSource } from "@talos/sdk-core";
import { getTalosClient } from "@/shared/api/client";

/** The mounted workspace tab owns the grant. Switching sessions keeps it alive;
 * closing the tab or changing account releases it through the captured client. */
export function useFilePreview(source: FilePreviewSource, scope: string, instanceKey: string) {
  const [revision, setRevision] = useState(0);
  const fileId = source.kind === "file" ? source.fileId : null;
  const sessionId = source.kind === "sessionPath" ? source.sessionId : null;
  const path = source.kind === "sessionPath" ? source.path : null;
  const key = JSON.stringify([scope, instanceKey, fileId, sessionId, path, revision]);
  const [state, setState] = useState<{ key: string; descriptor?: FilePreviewDescriptor; error?: unknown }>();
  useEffect(() => {
    const client = getTalosClient().files.preview;
    let disposed = false;
    let prepared: FilePreviewDescriptor | undefined;
    const release = (descriptor: FilePreviewDescriptor) => void client.close({ previewId: descriptor.previewId }).catch(error => console.warn("Preview release failed", error));
    const source: FilePreviewSource = fileId !== null ? { kind: "file", fileId } : { kind: "sessionPath", sessionId: sessionId!, path: path! };
    void client.prepare({ source }).then(descriptor => {
      if (disposed) release(descriptor);
      else { prepared = descriptor; setState({ key, descriptor }); }
    }, error => { if (!disposed) setState({ key, error }); });
    return () => { disposed = true; if (prepared) release(prepared); };
  }, [key, fileId, sessionId, path]);
  const current = state?.key === key ? state : undefined;
  return { descriptor: current?.descriptor, error: current?.error, pending: !current, refresh: () => setRevision(value => value + 1) };
}

export function usePreviewContent(descriptor: FilePreviewDescriptor | undefined, enabled: boolean, resourcePath = descriptor?.entryPath) {
  return useQuery({
    queryKey: ["talos", "preview-content", descriptor?.previewId, resourcePath],
    queryFn: () => getTalosClient().files.preview.content(descriptor!, resourcePath),
    enabled: !!descriptor && enabled, retry: false, refetchOnWindowFocus: false,
  });
}

/** Archive listings use filename hints; inspect only the selected member through
 * the common metadata-only read, without copying other members into the UI. */
export function usePreviewMetadata(descriptor: FilePreviewDescriptor | undefined, resourcePath: string | undefined) {
  return useQuery({
    queryKey: ["talos", "preview-metadata", descriptor?.previewId, resourcePath],
    queryFn: async () => (await getTalosClient().files.preview.readBytes({ previewId: descriptor!.previewId, resourcePath: resourcePath!, offset: 0, limit: 0 })).metadata,
    enabled: !!descriptor && resourcePath !== undefined, retry: false, refetchOnWindowFocus: false,
  });
}
