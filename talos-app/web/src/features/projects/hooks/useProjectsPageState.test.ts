import { describe, expect, it } from "vitest";

import { filterProjects } from "./useProjectsPageState";

describe("useProjectsPageState helpers", () => {
  it("sorts projects by activity and filters by query", () => {
    const projects = [
      {
        id: "project-1",
        name: "alpha",
        directory: "/srv/alpha",
        createdAt: 1,
        updatedAt: 10,
        sessionCount: 1,
      },
      {
        id: "project-2",
        name: "beta",
        directory: "/srv/beta",
        createdAt: 2,
        updatedAt: 20,
        sessionCount: 2,
      },
    ];

    const filtered = filterProjects(projects, "bet", "activity");

    expect(filtered).toHaveLength(1);
    expect(filtered[0]?.id).toBe("project-2");
  });

  it("sorts projects by creation date when requested", () => {
    const projects = [
      {
        id: "project-1",
        name: "alpha",
        directory: "/srv/alpha",
        createdAt: 1,
        updatedAt: 20,
        sessionCount: 5,
      },
      {
        id: "project-2",
        name: "beta",
        directory: "/srv/beta",
        createdAt: 3,
        updatedAt: 10,
        sessionCount: 1,
      },
    ];

    const filtered = filterProjects(projects, "", "date_created");

    expect(filtered.map((project) => project.id)).toEqual(["project-2", "project-1"]);
  });
});
