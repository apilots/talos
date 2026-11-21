import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import type { FilePreviewDescriptor, FilePreviewSource } from "@talos/sdk-core";
import { getTalosClient } from "@/shared/api/client";

/** The mounted workspace tab owns the grant. Switching sessions keeps it alive;
 * closing the tab or changing account releases it through the captured client. */
export function useFilePreview(source: FilePreviewSource, scope: string, instanceKey: string, owner: { sessionId?: string; nodeId?: string }) {
  const [revision, setRevision] = useState(0);
  const fileId = source.kind === "file" ? source.fileId : null;
  const sessionId = source.kind === "sessionPath" ? source.sessionId : owner.sessionId;
  const nodeId = sessionId ? undefined : owner.nodeId;
  const path = source.kind === "sessionPath" ? source.path : null;
  const key = JSON.stringify([scope, instanceKey, fileId, sessionId, path, nodeId, revision]);
  const [state, setState] = useState<{ key: string; descriptor?: FilePreviewDescriptor; error?: unknown }>();
  useEffect(() => {
    let disposed = false;
    let release: (() => void) | undefined;
    const prepare = async () => {
      const { files } = await getTalosClient({ service: "files", sessionId, nodeId });
      if (disposed) return;
      const source: FilePreviewSource = fileId !== null ? { kind: "file", fileId } : { kind: "sessionPath", sessionId: sessionId!, path: path! };
      const descriptor = await files.preview.prepare({ source });
      release = () => void files.preview.close({ previewId: descriptor.previewId }).catch(error => console.warn("Preview release failed", error));
      if (disposed) release();
      else setState({ key, descriptor });
    };
    void prepare().catch(error => { if (!disposed) setState({ key, error }); });
    return () => { disposed = true; release?.(); };
  }, [key, fileId, sessionId, path, nodeId]);
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
