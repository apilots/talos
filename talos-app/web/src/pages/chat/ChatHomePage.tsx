import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { automationRequestError, prepareAutomationMessage } from "@/features/automation";
import { getTalosClient } from "@/shared/api/client";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";

import {
  ChatComposer,
  generateSessionId,
  createInitialTurnRequest,
  startTimelineTurn,
  modelSelectionForRef,
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
import { useComputers, useManagedWorkerStatus, useProductScope } from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { resolveProjectView } from "@/shared/types/domain";
import type {
  AutomationMode,
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
  const { status: managedStatus } = useManagedWorkerStatus();
  const instance = managedStatus.data?.instance;
  const managedPhase = instance?.nodeId === computers.selectedId
    ? instance.phase : computers.selected?.managedWorkerPhase;
  const canSend = managedPhase == null
    ? computers.selected?.available
    : managedPhase === "ready" || managedPhase === "sleeping";
  const scope = useProductScope();
  const draftKey = JSON.stringify([scope, computers.selectedId, "home"]);
  const admission = useMemo(() => ({ draftKey, sessionId: generateSessionId() }), [draftKey]);
  const submission = useRef<{ body: string; requestId: string } | undefined>(undefined);
  const ownerRef = useRef<typeof admission | undefined>(admission);
  ownerRef.current = admission;
  useEffect(() => { ownerRef.current = admission; return () => { ownerRef.current = undefined; }; }, [admission]);
  const commands = useQuery({ queryKey: ["talos", scope, "automation-commands"],
    queryFn: async () => (await getTalosClient({ nodeId: computers.selectedId })).automation.commandCatalog({}),
    enabled: canSend, staleTime: Infinity });
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

  async function handleStartSession(
    initialPrompt?: string,
    attachments: FileInfo[] = [],
    mode?: AutomationMode,
  ) {
    if (!canSend) return;
    const prepared = await prepareAutomationMessage(initialPrompt ?? "", mode, computers.selectedId, managedPhase != null);
    if (ownerRef.current !== admission) return;
    if (prepared.kind === "management") throw automationRequestError("chatRequired");
    const trimmed = prepared.prompt.trim();
    const request = createInitialTurnRequest(admission.sessionId, trimmed, {
      nodeId: computers.selectedId,
      skillSelection: prepared.skillSelection,
      fileIds: attachments.map(file => file.fileId),
      modelSelection: modelSelectionForRef(models, selectedModel, selectedReasoningLevel),
      userPermissionMode: permissionMode,
      project: activeProject,
    });
    const body = JSON.stringify(request);
    if (submission.current?.body !== body) submission.current = { body, requestId: crypto.randomUUID() };
    request.clientRequestId = submission.current.requestId;
    await startTimelineTurn(request);
    // Clear only the input acknowledged by the service.
    if (useAppStore.getState().composerDrafts[admission.draftKey] === initialPrompt) saveDraft(admission.draftKey, "");
    if (ownerRef.current === admission) navigate(`/sessions/${admission.sessionId}`);
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
        <h1 className="mb-8 text-center text-[26px] font-semibold tracking-tight text-ink">
          {t("chat:startPromptTitle")}
        </h1>
        <ChatComposer
          key={draftKey}
          nodeId={computers.selectedId}
          automationCommands={commands.data?.data}
          activeProjectId={activeProject?.id ?? activeProjectId}
          disabled={!canSend}
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
          onSend={(attachments, mode) => handleStartSession(draft, attachments, mode)}
          permissionMode={permissionMode}
          projects={projects}
          selectedModel={selectedModel ? modelRefKey(selectedModel) : undefined}
          reasoningLevel={selectedReasoningLevel}
          supportedAttachmentKinds={supportedAttachmentKinds(selectedModelInfo)}
          showProjectSelector
        />
      </div>
    </div>
  );
}
