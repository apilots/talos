import { useMemo, type ClipboardEvent } from "react";
import type { FileInfo, SkillSuggestion } from "@talos/sdk-browser";

import { isUnboundProject } from "@/shared/types/domain";
import {
  useComposerAttachments,
  type ComposerAttachmentKind,
} from "./useComposerAttachments";

interface ChatComposerAttachmentDraftOptions {
  activeProjectId: string;
  initialAttachments: FileInfo[];
  supportedKinds: ComposerAttachmentKind[];
}

export function useChatComposerAttachmentDraft({
  activeProjectId,
  initialAttachments,
  supportedKinds,
}: ChatComposerAttachmentDraftOptions) {
  const scope = useMemo(
    () =>
      isUnboundProject(activeProjectId)
        ? ({ type: "personal" } as const)
        : ({ type: "project", projectId: activeProjectId } as const),
    [activeProjectId],
  );
  const draft = useComposerAttachments({
    scope,
    supportedKinds,
    initialAttachments,
  });

  function canSubmit(text: string): boolean {
    return (
      (text.trim().length > 0 || draft.fileIds.length > 0) &&
      !draft.hasInvalidAttachments &&
      !draft.hasPendingUploads &&
      !draft.hasFailedUploads
    );
  }

  function handlePaste(event: ClipboardEvent<HTMLTextAreaElement>): void {
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
    skills: SkillSuggestion[],
    onSend: (skills: SkillSuggestion[], attachments: FileInfo[]) => Promise<void> | void,
  ): Promise<void> {
    if (!canSubmit(text)) {
      return;
    }
    await onSend(skills, draft.attachments);
    draft.clear();
  }

  return { ...draft, canSubmit, handlePaste, submit };
}
