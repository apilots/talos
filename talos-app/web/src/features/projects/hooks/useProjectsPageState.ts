import { useMemo, useState } from "react";

import { useProjectsQuery } from "@/features/chat";
import type { ProjectRecord } from "@/shared/types/ui";

export type ProjectSort = "activity" | "last_edited" | "date_created";

export function filterProjects(projects: ProjectRecord[], query: string, sort: ProjectSort) {
  const normalizedQuery = query.trim().toLowerCase();
  return projects
    .slice()
    .sort((left, right) => {
      if (sort === "activity") {
        const countDiff = (right.sessionCount ?? 0) - (left.sessionCount ?? 0);
        if (countDiff !== 0) {
          return countDiff;
        }
        return right.updatedAt - left.updatedAt;
      }
      if (sort === "date_created") {
        return right.createdAt - left.createdAt;
      }
      return right.updatedAt - left.updatedAt;
    })
    .filter((project) => {
      if (!normalizedQuery) {
        return true;
      }
      return [project.name, project.directory ?? ""].some((field) =>
        field.toLowerCase().includes(normalizedQuery),
      );
    });
}

export function useProjectsPageState() {
  const projectsQuery = useProjectsQuery();
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<ProjectSort>("activity");

  const filteredProjects = useMemo(
    () => filterProjects(projectsQuery.data ?? [], query, sort),
    [projectsQuery.data, query, sort],
  );

  return {
    filteredProjects,
    projectsQuery,
    query,
    sort,
    setQuery,
    setSort,
  };
}
