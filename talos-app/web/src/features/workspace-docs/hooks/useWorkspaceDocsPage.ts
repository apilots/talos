import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import type { WorkspaceDocInfo, WorkspaceDocKind } from "@talos/sdk-browser";

import { useEnvironmentKey } from "@/features/computers";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

const DOC_KINDS: WorkspaceDocKind[] = ["soul", "identity", "user", "heartbeat", "memory"];

function emptyDoc(kind: WorkspaceDocKind): WorkspaceDocInfo {
  return {
    kind,
    path: `${kind.toUpperCase()}.md`,
    exists: false,
    content: "",
    updated_at: null,
    version_token: null,
  };
}

export function formatWorkspaceDocDate(updatedAt?: number | null) {
  if (!updatedAt) {
    return "Not saved yet";
  }
  const locale = typeof navigator !== "undefined" ? navigator.language : "en";
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(updatedAt);
}

export function useWorkspaceDocsPage() {
  const connectionKey = useEnvironmentKey();
  const queryClient = useQueryClient();
  const [selectedDocKind, setSelectedDocKind] = useState<WorkspaceDocKind>("memory");
  const [docDraft, setDocDraft] = useState("");

  const docDetailQuery = useQuery({
    queryKey: ["talos", connectionKey, "workspace-doc", selectedDocKind],
    queryFn: async () => {
      const response = await getTalosClient().workspaceDocs.get(selectedDocKind);
      return response.doc;
    },
    refetchOnWindowFocus: false,
  });

  useEffect(() => {
    setDocDraft(docDetailQuery.data?.content ?? "");
  }, [docDetailQuery.data?.kind, docDetailQuery.data?.version_token, docDetailQuery.data?.content]);

  const saveMutation = useMutation({
    mutationFn: async () => {
      const detail = docDetailQuery.data ?? emptyDoc(selectedDocKind);
      const baseVersion = detail.version_token
        ? { mode: "match" as const, version_token: detail.version_token }
        : { mode: "must_not_exist" as const };
      const response = await getTalosClient().workspaceDocs.put(selectedDocKind, {
        content: docDraft,
        base_version: baseVersion,
      });
      return response.doc;
    },
    onSuccess: async (detail) => {
      queryClient.setQueryData(["talos", connectionKey, "workspace-doc", detail.kind], detail);
    },
  });

  const docLabels = useMemo<Record<WorkspaceDocKind, string>>(
    () => ({
      soul: "SOUL.md",
      identity: "IDENTITY.md",
      user: "USER.md",
      heartbeat: "HEARTBEAT.md",
      memory: "MEMORY.md",
    }),
    [],
  );

  const selectedDetail = docDetailQuery.data ?? emptyDoc(selectedDocKind);

  return {
    docKinds: DOC_KINDS,
    docLabels,
    selectedDocKind,
    setSelectedDocKind,
    selectedDetail,
    docDraft,
    setDocDraft,
    saveCurrent: () => saveMutation.mutateAsync(),
    isDirty: docDraft !== selectedDetail.content,
    isLoading: docDetailQuery.isLoading,
    isSaving: saveMutation.isPending,
    loadError: docDetailQuery.error
      ? userVisibleErrorMessage(docDetailQuery.error, "Failed to load workspace document.")
      : null,
    saveError: saveMutation.error
      ? userVisibleErrorMessage(saveMutation.error, "Failed to save workspace document.")
      : null,
  };
}
