import { useEffect, useRef } from "react";
import type { ModelInfo } from "@talos/sdk-browser";

import { modelSelectionForRef, type SendTextOptions } from "@/features/chat";
import type { ProjectView } from "@/shared/types/ui";
import type { SessionLocationState } from "./chatSessionLocation";

type SendText = (text: string, options?: SendTextOptions) => Promise<void>;
type Navigate = (
  pathname: string,
  options: { replace: true; state: null },
) => void;

interface InitialSessionInputParams {
  locationPathname: string;
  locationState: SessionLocationState | null;
  models?: ModelInfo[];
  navigate: Navigate;
  projects: ProjectView[];
  sendText: SendText;
  sessionId: string;
  setDraft: (draft: string) => void;
}

export function hasInitialSessionInput(state: SessionLocationState | null): boolean {
  return Boolean(state?.initialPrompt?.trim() || state?.initialAttachments?.length);
}

/** Sends navigation-provided text and file references exactly once for a draft session. */
export function useInitialSessionInput({
  locationPathname,
  locationState,
  models,
  navigate,
  projects,
  sendText,
  sessionId,
  setDraft,
}: InitialSessionInputParams): void {
  const handledRef = useRef(false);
  const mountedRef = useRef(false);
  const activeSessionIdRef = useRef(sessionId);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    activeSessionIdRef.current = sessionId;
  }, [sessionId]);

  useEffect(() => {
    if (!hasInitialSessionInput(locationState) || handledRef.current) {
      return;
    }
    handledRef.current = true;

    const initialPrompt = locationState?.initialPrompt?.trim() ?? "";
    const initialAttachments = locationState?.initialAttachments ?? [];
    const project =
      locationState?.initialProject ??
      projects.find((entry) => entry.id === locationState?.initialProjectId);
    void sendText(initialPrompt, {
      modelSelection: modelSelectionForRef(
        models,
        locationState?.initialModel,
        locationState?.initialReasoningLevel,
      ),
      userPermissionMode: locationState?.initialPermissionMode,
      project,
      skills: locationState?.initialSkills?.length
        ? locationState.initialSkills
        : undefined,
      ...(initialAttachments.length
        ? { fileIds: initialAttachments.map((file) => file.fileId) }
        : {}),
    })
      .then(() => {
        if (mountedRef.current && activeSessionIdRef.current === sessionId) {
          navigate(locationPathname, { replace: true, state: null });
        }
      })
      .catch(() => {
        if (mountedRef.current && activeSessionIdRef.current === sessionId) {
          setDraft(initialPrompt);
        }
      });
  }, [
    locationPathname,
    locationState,
    models,
    navigate,
    projects,
    sendText,
    sessionId,
    setDraft,
  ]);
}
