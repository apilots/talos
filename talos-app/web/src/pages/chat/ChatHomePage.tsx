import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";

import {
  ChatComposer,
  generateSessionId,
  modelInfoForRef,
  modelPickerOptions,
  modelRefForKey,
  modelRefKey,
  reasoningLevelForModel,
  useCreateProjectMutation,
  useModelsQuery,
  useProjectsView,
} from "@/features/chat";
import { useLlmConfigQuery } from "@/features/settings";
import { useAppStore } from "@/shared/store/app-store";
import { resolveProjectView } from "@/shared/types/domain";
import type { ModelRef, SkillSuggestion } from "@talos/sdk-browser";
import type { ReasoningLevel } from "@/shared/types/ui";

interface HomeLocationState {
  initialDraft?: string;
  initialSkills?: SkillSuggestion[];
}

function initialSkillsState(skills: SkillSuggestion[]) {
  return skills.length > 0 ? skills : undefined;
}

export function ChatHomePage() {
  const { t } = useTranslation(["chat"]);
  const navigate = useNavigate();
  const location = useLocation();
  const locationState = location.state as HomeLocationState | null;
  const { data: models } = useModelsQuery();
  const { data: llmConfigRead } = useLlmConfigQuery();
  const configuredModel = llmConfigRead?.config.model;
  const configuredProvider = llmConfigRead?.config.provider;
  const projectsView = useProjectsView();
  const createProject = useCreateProjectMutation();
  const projects = projectsView.projects;
  const activeProjectId = projectsView.activeProjectId;
  const setActiveProjectId = projectsView.setActiveProjectId;
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const [draft, setDraft] = useState(locationState?.initialDraft ?? "");
  const [initialSkills] = useState<SkillSuggestion[]>(locationState?.initialSkills ?? []);
  const [selectedModel, setSelectedModel] = useState<ModelRef>();
  const [selectedReasoningLevel, setSelectedReasoningLevel] = useState<ReasoningLevel>("none");
  const selectedModelInfo = modelInfoForRef(models, selectedModel);

  const activeProject = useMemo(
    () => resolveProjectView(projects, activeProjectId),
    [activeProjectId, projects],
  );

  useEffect(() => {
    const configuredModelRef =
      configuredProvider && configuredModel
        ? { provider: configuredProvider, id: configuredModel }
        : undefined;
    if (!selectedModel && modelInfoForRef(models, configuredModelRef)) {
      setSelectedModel(configuredModelRef);
      setSelectedReasoningLevel(reasoningLevelForModel(models, configuredModelRef));
    }
  }, [configuredModel, configuredProvider, models, selectedModel]);

  useEffect(() => {
    if (activeProject && activeProject.id !== activeProjectId) {
      setActiveProjectId(activeProject.id);
    }
  }, [activeProject, activeProjectId, setActiveProjectId]);

  useEffect(() => {
    if (!locationState?.initialDraft && !locationState?.initialSkills?.length) {
      return;
    }
    navigate(location.pathname, { replace: true, state: null });
  }, [location.pathname, locationState?.initialDraft, locationState?.initialSkills, navigate]);

  async function handleStartDraft(initialPrompt?: string, skills: SkillSuggestion[] = []) {
    const trimmed = initialPrompt?.trim();
    const sessionId = generateSessionId();
    navigate(`/sessions/${sessionId}`, {
      state: trimmed
        ? {
            initialPrompt: trimmed,
            initialModel: selectedModel,
            initialReasoningLevel: selectedReasoningLevel,
            initialProjectId: activeProject?.id,
            initialProject: activeProject,
            initialSkills: initialSkillsState(skills),
          }
        : undefined,
    });
  }

  async function handleCreateProject(input: string | { directory: string; name?: string }) {
    const project = await createProject.mutateAsync(input);
    setActiveProjectId(project.id);
  }

  return (
    <div className="flex h-full flex-col items-center justify-center px-8">
      <div className="w-full max-w-[660px]">
        <h1 className="mb-8 text-center text-[26px] font-semibold tracking-tight text-ink">
          {t("chat:startPromptTitle")}
        </h1>
        <ChatComposer
          activeProjectId={activeProject?.id ?? activeProjectId}
          disabled={false}
          draft={draft}
          isStreaming={false}
          models={modelPickerOptions(models)}
          onCreateProject={handleCreateProject}
          onDraftChange={setDraft}
          initialSelectedSkills={initialSkills}
          onModelChange={(model) => {
            const modelRef = modelRefForKey(model);
            if (!modelRef) {
              return;
            }
            setSelectedModel(modelRef);
            setSelectedReasoningLevel(reasoningLevelForModel(models, modelRef));
          }}
          onProjectChange={setActiveProjectId}
          onRuntimeModeChange={(mode) => {
            if (mode === "local") {
              setConnection({ runtimeMode: mode });
            }
          }}
          onReasoningLevelChange={setSelectedReasoningLevel}
          onSend={(skills) => handleStartDraft(draft, skills)}
          projects={projects}
          runtimeMode={connection.runtimeMode}
          selectedModel={selectedModel ? modelRefKey(selectedModel) : undefined}
          reasoningLevel={selectedReasoningLevel}
          reasoningEnabled={selectedModelInfo?.reasoning ?? false}
          showProjectSelector
        />
      </div>
    </div>
  );
}
