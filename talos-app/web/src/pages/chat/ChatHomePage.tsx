import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { FolderPlus } from "lucide-react";

import { useCreateSessionMutation, useModelsQuery } from "@/features/chat/hooks/useChatQueries";
import { ChatComposer } from "@/features/chat/components/ChatComposer";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { useAppStore } from "@/shared/store/app-store";
import {
  describeProjectRoot,
  getApprovalModeLabel,
  type ProjectRecord,
  type ProjectRoot,
} from "@/shared/types/domain";

export function ChatHomePage() {
  const navigate = useNavigate();
  const { data: models } = useModelsQuery();
  const createSession = useCreateSessionMutation();
  const projects = useAppStore((state) => state.projects);
  const activeProjectId = useAppStore((state) => state.activeProjectId);
  const addProject = useAppStore((state) => state.addProject);
  const setActiveProjectId = useAppStore((state) => state.setActiveProjectId);
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const [draft, setDraft] = useState("");
  const [customPath, setCustomPath] = useState("");
  const [selectedModel, setSelectedModel] = useState<string | undefined>(models?.[0]?.id);

  const activeProject = useMemo(
    () => projects.find((project) => project.id === activeProjectId) ?? projects[0],
    [activeProjectId, projects],
  );

  useEffect(() => {
    if (!selectedModel && models?.[0]?.id) {
      setSelectedModel(models[0].id);
    }
  }, [models, selectedModel]);

  async function handleCreateSession(initialPrompt?: string) {
    const session = await createSession.mutateAsync(selectedModel ?? models?.[0]?.id);
    navigate(`/sessions/${session.context.session_id}`, {
      state: initialPrompt?.trim()
        ? {
            initialPrompt,
          }
        : undefined,
    });
  }

  function handleCreateProject(root: ProjectRoot) {
    const now = Date.now();
    const project: ProjectRecord = {
      id: `project-${now}`,
      name: root.type === "managed_workspace" ? "Server workspace" : root.path.split("/").at(-1) || "project",
      root,
      createdAt: now,
      lastUsedAt: now,
    };
    addProject(project);
    setActiveProjectId(project.id);
  }

  return (
    <div className="relative flex h-full overflow-y-auto px-8 py-8">
      <div className="pointer-events-none absolute inset-0 opacity-80">
        <div className="absolute left-[14%] top-[14%] h-40 w-40 rounded-full bg-brand/10 blur-3xl" />
        <div className="absolute bottom-[10%] right-[16%] h-56 w-56 rounded-full bg-info/8 blur-3xl" />
      </div>
      <div className="relative mx-auto my-auto w-full max-w-[820px]">
        <div className="mb-4 text-center">
          <div className="text-[11px] uppercase tracking-[0.22em] text-brand/80">New thread</div>
          <h1 className="mt-3 font-display text-[29px] font-semibold tracking-[-0.05em] text-ink">
            Start with one clear prompt
          </h1>
          <div className="mt-3 flex items-center justify-center gap-2 text-xs text-muted">
            <span className="rounded-full border border-border/70 bg-card/80 px-3 py-1 text-ink">
              {activeProject.name}
            </span>
            <span className="truncate rounded-full border border-border/70 bg-background/55 px-3 py-1">
              {describeProjectRoot(activeProject.root)}
            </span>
          </div>
        </div>

        <div className="mb-3 rounded-[1.35rem] border border-border/70 bg-card/72 p-2.5 backdrop-blur">
          <div className="grid gap-2.5 md:grid-cols-[minmax(0,0.95fr)_minmax(0,1.2fr)_auto]">
            <select
              className="w-full rounded-2xl border border-border bg-card/92 px-4 py-2.5 text-sm text-ink outline-none transition focus:border-brand/50"
              onChange={(event) => setActiveProjectId(event.target.value)}
              value={activeProjectId}
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.name === "Default workspace"
                    ? project.name
                    : `${project.name} · ${describeProjectRoot(project.root)}`}
                </option>
              ))}
            </select>
            <Input
              onChange={(event) => setCustomPath(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  const path = customPath.trim() || __TALOS_DEFAULT_PROJECT_ROOT__;
                  handleCreateProject({ type: "server_visible_path", path });
                  setCustomPath("");
                }
              }}
              placeholder={__TALOS_DEFAULT_PROJECT_ROOT__}
              value={customPath}
            />
            <Button
              onClick={() => {
                const path = customPath.trim() || __TALOS_DEFAULT_PROJECT_ROOT__;
                handleCreateProject({ type: "server_visible_path", path });
                setCustomPath("");
              }}
              type="button"
              variant="secondary"
            >
              <FolderPlus size={16} />
              Add path
            </Button>
          </div>
        </div>

        <div className="space-y-2">
          <ChatComposer
            activeProjectId={activeProjectId}
            approvalMode={connection.defaultApprovalMode}
            disabled={false}
            draft={draft}
            isStreaming={false}
            models={(models ?? []).map((model) => ({ id: model.id, name: model.name }))}
            onApprovalModeChange={(mode) => setConnection({ defaultApprovalMode: mode })}
            onDraftChange={setDraft}
            onModelChange={setSelectedModel}
            onProjectChange={setActiveProjectId}
            onRuntimeModeChange={(mode) => {
              if (mode === "cloud") {
                setConnection({ runtimeMode: mode });
              }
            }}
            onSend={() => handleCreateSession(draft)}
            projects={projects}
            runtimeMode={connection.runtimeMode}
            selectedModel={selectedModel}
            showProjectSelector={false}
          />
          <div className="pl-2 text-[10px] text-muted">
            First session uses {getApprovalModeLabel(connection.defaultApprovalMode)}.
          </div>
        </div>
      </div>
    </div>
  );
}
