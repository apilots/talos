import { useMemo, type ClipboardEvent } from "react";
import type { FileInfo } from "@talos/sdk-browser";

import { isUnboundProject } from "@/shared/types/domain";
import {
  useComposerAttachments,
  type ComposerAttachmentKind,
} from "./useComposerAttachments";

interface ChatComposerAttachmentDraftOptions {
  nodeId?: string;
  activeProjectId: string;
  supportedKinds: ComposerAttachmentKind[];
  initialFileIds?: string[];
  onChange?: () => void;
}

export function useChatComposerAttachmentDraft({
  nodeId,
  activeProjectId,
  supportedKinds,
  initialFileIds, onChange,
}: ChatComposerAttachmentDraftOptions) {
  const scope = useMemo(
    () =>
      isUnboundProject(activeProjectId)
        ? ({ type: "personal" } as const)
        : ({ type: "project", projectId: activeProjectId } as const),
    [activeProjectId],
  );
  const draft = useComposerAttachments({
    nodeId,
    scope,
    supportedKinds,
    initialFileIds, onChange,
    });

  function canSubmit(text: string): boolean {
    return (
      (text.trim().length > 0 || draft.fileIds.length > 0) &&
      !draft.hasInvalidAttachments &&
      !draft.hasPendingUploads &&
      !draft.hasFailedUploads
    );
  }

  function handlePaste(event: ClipboardEvent<HTMLElement>): void {
    const images = Array.from(event.clipboardData.files).filter((file) =>
      file.type.startsWith("image/"),
    );
    if (images.length === 0) {
      return;
    }
    event.preventDefault();
    void draft.upload(images).catch(() => undefined);
  }

  async function submit(
    text: string,
    onSend: (attachments: Pick<FileInfo, "fileId">[]) => Promise<void> | void,
  ): Promise<void> {
    if (!canSubmit(text)) {
      return;
    }
    await onSend(draft.attachments);
    draft.clear();
  }

  return { ...draft, canSubmit, handlePaste, submit };
}
