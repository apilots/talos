import { useMemo } from "react";

import { UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";

interface SelectedSessionProjectOptions {
  activeProjectId: string;
  draftProjectId?: string;
  projects: ProjectView[];
  sessionExists: boolean;
  sessionProjectId?: string | null;
}

export function useSelectedSessionProject({
  activeProjectId,
  draftProjectId,
  projects,
  sessionExists,
  sessionProjectId,
}: SelectedSessionProjectOptions) {
  const selectedProjectId = useMemo(() => {
    if (!sessionExists) {
      return draftProjectId ?? activeProjectId;
    }
    return sessionProjectId ?? UNBOUND_PROJECT_ID;
  }, [activeProjectId, draftProjectId, sessionExists, sessionProjectId]);
  const boundProject = useMemo(
    () => projects.find((project) => project.id === sessionProjectId),
    [projects, sessionProjectId],
  );

  return { boundProject, selectedProjectId };
}
