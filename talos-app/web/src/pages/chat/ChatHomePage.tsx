import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { automationRequestError, prepareAutomationMessage } from "@/features/automation";
import { getTalosClient } from "@/shared/api/client";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";

import {
  ChatComposer,
  generateSessionId,
  restoreComposerRequest,
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
import { resolveProjectView, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import type {
  AutomationMode,
  FileInfo,
  ModelRef,
  StartTurnRequest,
  UserPermissionMode,
} from "@talos/sdk-browser";
import type { ReasoningLevel } from "@/shared/types/ui";

interface HomeLocationState {
  initialDraft?: string;
}

export function ChatHomePage() {
  const computers = useComputers();
  const scope = useProductScope();
  const draftKey = JSON.stringify([scope, computers.selectedId, "home"]);
  return <ChatHomeForm key={draftKey} {...{ computers, scope, draftKey }} />;
}

function ChatHomeForm({ computers, scope, draftKey }: {
  computers: ReturnType<typeof useComputers>; scope: string; draftKey: string;
}) {
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
  const { status: managedStatus } = useManagedWorkerStatus();
  const instance = managedStatus.data?.instance;
  const managedPhase = instance?.nodeId === computers.selectedId
    ? instance.phase : computers.selected?.managedWorkerPhase;
  const canSend = managedPhase == null
    ? computers.selected?.available
    : managedPhase === "ready" || managedPhase === "sleeping";
  const [restored] = useState<StartTurnRequest | undefined>(() => {
    const pending = useAppStore.getState().composerRequests[draftKey];
    return pending ? restoreComposerRequest(pending.body) : undefined;
  });
  const restoredFileIds = useMemo(() => restored?.input.content.flatMap(item => item.type === "file" ? [item.fileId] : []) ?? [], [restored]);
  const restoredMode = restored?.input.content.find(item => item.type === "skillSelection")?.name;
  const [projectOverride, setProjectOverride] = useState(restored?.projectId);
  const selectedProjectId = projectOverride === undefined ? activeProjectId : projectOverride ?? UNBOUND_PROJECT_ID;
  const edits = useRef(0);
  const editSubmission = () => { edits.current += 1; useAppStore.getState().saveComposerRequest(draftKey); };
  const admission = useMemo(() => ({ draftKey }), [draftKey]);
  const ownerRef = useRef<typeof admission | undefined>(admission);
  ownerRef.current = admission;
  useEffect(() => { ownerRef.current = admission; return () => { ownerRef.current = undefined; }; }, [admission]);
  const commands = useQuery({ queryKey: ["talos", scope, "automation-commands", computers.selectedId],
    queryFn: async () => (await getTalosClient({ nodeId: computers.selectedId })).automation.commandCatalog({}),
    enabled: canSend, staleTime: Infinity });
  const savedDraft = useAppStore((state) => state.composerDrafts[draftKey] ?? "");
  const saveDraft = useAppStore((state) => state.saveComposerDraft);
  const draft = locationState?.initialDraft ?? savedDraft;
  const setDraft = (text: string) => { edits.current += 1; saveDraft(draftKey, text); };
  const [selectedModel, setSelectedModel] = useState<ModelRef | undefined>(restored?.modelSelection?.modelRef);
  const [selectedReasoningLevel, setSelectedReasoningLevel] = useState<ReasoningLevel>(reasoningLevelForEffort(restored?.modelSelection?.reasoningEffort ?? null));
  const [permissionMode, setPermissionMode] = useState<UserPermissionMode>(restored?.userPermissionMode ?? "fullAccess");
  const selectedModelInfo = modelInfoForRef(models, selectedModel);

  const activeProject = useMemo(
    () => resolveProjectView(projects, selectedProjectId),
    [selectedProjectId, projects],
  );

  useEffect(() => {
    const defaultSelection = modelCatalog?.defaultModelSelection;
    if (!selectedModel && defaultSelection) {
      setSelectedModel(defaultSelection.modelRef);
      setSelectedReasoningLevel(reasoningLevelForEffort(defaultSelection.reasoningEffort));
    }
  }, [modelCatalog, selectedModel]);

  useEffect(() => {
    if (projectOverride === undefined && activeProject && activeProject.id !== activeProjectId) {
      setActiveProjectId(activeProject.id);
    }
  }, [activeProject, activeProjectId, projectOverride, setActiveProjectId]);

  useEffect(() => {
    if (!locationState?.initialDraft) {
      return;
    }
    saveDraft(draftKey, locationState.initialDraft);
    void navigate(location.pathname, { replace: true, state: null });
  }, [draftKey, saveDraft, location.pathname, locationState?.initialDraft, navigate]);

  async function handleStartSession(
    initialPrompt?: string,
    attachments: Pick<FileInfo, "fileId">[] = [],
    mode?: AutomationMode,
  ) {
    if (!canSend) return;
    const revision = edits.current;
    const pending = useAppStore.getState().composerRequests[admission.draftKey];
    const sessionId = pending?.sessionId ?? generateSessionId();
    let request: StartTurnRequest;
    if (pending) request = restoreComposerRequest(pending.body);
    else {
      const prepared = await prepareAutomationMessage(initialPrompt ?? "", mode, computers.selectedId, managedPhase != null);
      if (ownerRef.current !== admission) return;
      if (prepared.kind === "management") throw automationRequestError("chatRequired");
      request = createInitialTurnRequest(sessionId, prepared.prompt.trim(), {
        nodeId: computers.selectedId, skillSelection: prepared.skillSelection,
        fileIds: attachments.map(file => file.fileId),
        modelSelection: modelSelectionForRef(models, selectedModel, selectedReasoningLevel),
        userPermissionMode: permissionMode, project: activeProject,
      });
      if (projectOverride !== undefined) request.projectId = projectOverride;
    }
    const submission = pending ?? { sessionId, body: JSON.stringify(request), requestId: generateSessionId() };
    if (edits.current === revision) useAppStore.getState().saveComposerRequest(admission.draftKey, submission);
    request.clientRequestId = submission.requestId;
    await startTimelineTurn(request);
    // Clear only the input acknowledged by the service.
    const current = useAppStore.getState();
    if (current.composerRequests[admission.draftKey]?.requestId === submission.requestId) {
      current.saveComposerRequest(admission.draftKey);
      if (current.composerDrafts[admission.draftKey] === initialPrompt) saveDraft(admission.draftKey, "");
    }
    if (ownerRef.current === admission) void navigate(`/sessions/${sessionId}`);
  }

  async function handleCreateProject(input: string | { directory: string; name?: string }) {
    const project = await createProject.mutateAsync(input);
    handleProjectChange(project.id);
  }

  function handleProjectChange(projectId: string) {
    if (projectId === selectedProjectId) return;
    editSubmission(); setProjectOverride(undefined); setActiveProjectId(projectId);
  }

  function handlePermissionModeChange(mode: UserPermissionMode) {
    if (mode === permissionMode) return;
    if (
      mode === "fullAccess" &&
      permissionMode !== "fullAccess" &&
      !window.confirm(t("chat:fullAccessConfirmation"))
    ) {
      return;
    }
    editSubmission(); setPermissionMode(mode);
  }

  return (
    <div className="flex h-full flex-col items-center justify-center px-8">
      <div className="w-full max-w-[660px]">
        <h1 className="mb-8 text-center text-[26px] font-semibold tracking-tight text-ink">
          {t("chat:startPromptTitle")}
        </h1>
        <ChatComposer
          key={draftKey}
          initialFileIds={restoredFileIds}
          initialMode={restoredMode === "goal" || restoredMode === "loop" ? restoredMode : undefined}
          onInputChange={editSubmission}
          nodeId={computers.selectedId}
          automationCommands={commands.data?.data}
          activeProjectId={projectOverride === undefined ? activeProject?.id ?? activeProjectId : selectedProjectId}
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
            editSubmission();
            setSelectedModel(modelRef);
            setSelectedReasoningLevel(reasoningLevelForModel(models, modelRef));
          }}
          onPermissionModeChange={handlePermissionModeChange}
          onProjectChange={handleProjectChange}
          onReasoningLevelChange={(level) => { editSubmission(); setSelectedReasoningLevel(level); }}
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
