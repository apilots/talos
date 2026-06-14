import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import {
  ChatComposer,
  generateSessionId,
  useCreateProjectMutation,
  useModelsQuery,
  useProjectsQuery,
} from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { resolveProjectRecord } from "@/shared/types/domain";

export function ChatHomePage() {
  const { t } = useTranslation(["chat"]);
  const navigate = useNavigate();
  const { data: models } = useModelsQuery();
  useProjectsQuery();
  const createProject = useCreateProjectMutation();
  const projects = useAppStore((state) => state.projects);
  const activeProjectId = useAppStore((state) => state.activeProjectId);
  const setActiveProjectId = useAppStore((state) => state.setActiveProjectId);
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const [draft, setDraft] = useState("");
  const [selectedModel, setSelectedModel] = useState<string | undefined>(models?.[0]?.id);

  const activeProject = useMemo(
    () => resolveProjectRecord(projects, activeProjectId),
    [activeProjectId, projects],
  );

  useEffect(() => {
    if (!selectedModel && models?.[0]?.id) {
      setSelectedModel(models[0].id);
    }
  }, [models, selectedModel]);

  useEffect(() => {
    if (activeProject && activeProject.id !== activeProjectId) {
      setActiveProjectId(activeProject.id);
    }
  }, [activeProject, activeProjectId, setActiveProjectId]);

  async function handleStartDraft(initialPrompt?: string) {
    const trimmed = initialPrompt?.trim();
    const sessionId = generateSessionId();
    navigate(`/sessions/${sessionId}`, {
      state: trimmed
        ? {
            initialPrompt: trimmed,
            initialModel: selectedModel ?? models?.[0]?.id,
            initialProjectId: activeProject?.id,
            initialProject: activeProject,
          }
        : undefined,
    });
  }

  async function handleCreateProject(path: string) {
    const project = await createProject.mutateAsync(path);
    setActiveProjectId(project.id);
  }

  return (
    <div className="flex h-full flex-col items-center justify-center px-8">
      <div className="w-full max-w-[660px]">
        <h1 className="mb-8 text-center text-[26px] font-semibold tracking-tight text-ink">
          {t("chat:startPromptTitle")}
        </h1>
        <div className="space-y-2">
          <ChatComposer
            activeProjectId={activeProject?.id ?? activeProjectId}
            disabled={false}
            draft={draft}
            isStreaming={false}
            models={(models ?? []).map((model) => ({ id: model.id, name: model.name }))}
            onCreateProject={handleCreateProject}
            onDraftChange={setDraft}
            onModelChange={setSelectedModel}
            onProjectChange={setActiveProjectId}
            onRuntimeModeChange={(mode) => {
              if (mode === "local") {
                setConnection({ runtimeMode: mode });
              }
            }}
            onSend={() => handleStartDraft(draft)}
            projects={projects}
            runtimeMode={connection.runtimeMode}
            selectedModel={selectedModel}
            showProjectSelector
          />
        </div>
      </div>
    </div>
  );
}
