import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";

import {
  ChatComposer,
  generateSessionId,
  supportedAttachmentKinds,
  modelInfoForRef,
  modelPickerOptions,
  modelRefForKey,
  modelRefKey,
  reasoningLevelForEffort,
  reasoningLevelForModel,
  useCreateProjectMutation,
  useModelsQuery,
  useProjectsView,
} from "@/features/chat";
import { useComputers, useProductScope, ComputerContext } from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { resolveProjectView } from "@/shared/types/domain";
import type {
  FileInfo,
  ModelRef,
  UserPermissionMode,
} from "@talos/sdk-browser";
import type { ReasoningLevel } from "@/shared/types/ui";

interface HomeLocationState {
  initialDraft?: string;
}

export function ChatHomePage() {
  const { t } = useTranslation(["chat"]);
  const navigate = useNavigate();
  const location = useLocation();
  const locationState = location.state as HomeLocationState | null;
  const { data: modelCatalog } = useModelsQuery();
  const models = modelCatalog?.models;
  const projectsView = useProjectsView();
  const createProject = useCreateProjectMutation();
  const projects = projectsView.projects;
  const activeProjectId = projectsView.activeProjectId;
  const setActiveProjectId = projectsView.setActiveProjectId;
  const computers = useComputers();
  const scope = useProductScope();
  const draftKey = JSON.stringify([scope, computers.selectedId, "home"]);
  const savedDraft = useAppStore((state) => state.composerDrafts[draftKey] ?? "");
  const saveDraft = useAppStore((state) => state.saveComposerDraft);
  const draft = locationState?.initialDraft ?? savedDraft;
  const setDraft = (text: string) => saveDraft(draftKey, text);
  const [selectedModel, setSelectedModel] = useState<ModelRef>();
  const [selectedReasoningLevel, setSelectedReasoningLevel] = useState<ReasoningLevel>("none");
  const [permissionMode, setPermissionMode] = useState<UserPermissionMode>("fullAccess");
  const selectedModelInfo = modelInfoForRef(models, selectedModel);

  const activeProject = useMemo(
    () => resolveProjectView(projects, activeProjectId),
    [activeProjectId, projects],
  );

  useEffect(() => {
    const defaultSelection = modelCatalog?.defaultModelSelection;
    if (!selectedModel && defaultSelection) {
      setSelectedModel(defaultSelection.modelRef);
      setSelectedReasoningLevel(reasoningLevelForEffort(defaultSelection.reasoningEffort));
    }
  }, [modelCatalog, selectedModel]);

  useEffect(() => {
    if (activeProject && activeProject.id !== activeProjectId) {
      setActiveProjectId(activeProject.id);
    }
  }, [activeProject, activeProjectId, setActiveProjectId]);

  useEffect(() => {
    if (!locationState?.initialDraft) {
      return;
    }
    saveDraft(draftKey, locationState.initialDraft);
    navigate(location.pathname, { replace: true, state: null });
  }, [draftKey, saveDraft, location.pathname, locationState?.initialDraft, navigate]);

  async function handleStartDraft(
    initialPrompt?: string,
    attachments: FileInfo[] = [],
  ) {
    if (!computers.selected?.available) return;
    const trimmed = initialPrompt?.trim();
    setDraft("");
    const sessionId = generateSessionId();
    navigate(`/sessions/${sessionId}`, {
      state: trimmed || attachments.length > 0
        ? {
            initialNodeId: computers.selectedId,
            initialPrompt: trimmed ?? "",
            initialModel: selectedModel,
            initialReasoningLevel: selectedReasoningLevel,
            initialPermissionMode: permissionMode,
            initialProjectId: activeProject?.id,
            initialProject: activeProject,
            initialAttachments: attachments.length > 0 ? attachments : undefined,
          }
        : undefined,
    });
  }

  async function handleCreateProject(input: string | { directory: string; name?: string }) {
    const project = await createProject.mutateAsync(input);
    setActiveProjectId(project.id);
  }

  function handlePermissionModeChange(mode: UserPermissionMode) {
    if (
      mode === "fullAccess" &&
      permissionMode !== "fullAccess" &&
      !window.confirm(t("chat:fullAccessConfirmation"))
    ) {
      return;
    }
    setPermissionMode(mode);
  }

  return (
    <div className="flex h-full flex-col items-center justify-center px-8">
      <div className="w-full max-w-[660px]">
        <ComputerContext />
        <h1 className="mb-8 text-center text-[26px] font-semibold tracking-tight text-ink">
          {t("chat:startPromptTitle")}
        </h1>
        <ChatComposer
          activeProjectId={activeProject?.id ?? activeProjectId}
          disabled={!computers.selected?.available}
          draft={draft}
          isStreaming={false}
          models={modelPickerOptions(models)}
          onCreateProject={handleCreateProject}
          onDraftChange={setDraft}
          onModelChange={(model) => {
            const modelRef = modelRefForKey(model);
            if (!modelRef) {
              return;
            }
            setSelectedModel(modelRef);
            setSelectedReasoningLevel(reasoningLevelForModel(models, modelRef));
          }}
          onPermissionModeChange={handlePermissionModeChange}
          onProjectChange={setActiveProjectId}
          onReasoningLevelChange={setSelectedReasoningLevel}
          onSend={(attachments) => handleStartDraft(draft, attachments)}
          permissionMode={permissionMode}
          projects={projects}
          selectedModel={selectedModel ? modelRefKey(selectedModel) : undefined}
          reasoningLevel={selectedReasoningLevel}
          reasoningEnabled={selectedModelInfo?.reasoning ?? false}
          supportedAttachmentKinds={supportedAttachmentKinds(selectedModelInfo)}
          showProjectSelector
        />
      </div>
    </div>
  );
}
