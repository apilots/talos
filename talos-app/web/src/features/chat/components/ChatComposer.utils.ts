import type { ProjectView } from "@/shared/types/ui";

export function filterComposerProjects(projects: ProjectView[], search: string) {
  if (!search) {
    return projects;
  }
  const query = search.toLowerCase();
  return projects.filter(
    (project) =>
      project.name.toLowerCase().includes(query) ||
      project.directory?.toLowerCase().includes(query),
  );
}
