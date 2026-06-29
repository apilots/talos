import { type ReactNode, lazy, Suspense } from "react";
import { Navigate, createBrowserRouter, createHashRouter, useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";

import { AppLayout } from "@/app/layouts/AppLayout";
import { useConnectionSettings } from "@/features/connection";

const ChatHomePage = lazy(() =>
  import("@/pages/chat/ChatHomePage").then((module) => ({ default: module.ChatHomePage })),
);
const ChatSessionPage = lazy(() =>
  import("@/pages/chat/ChatSessionPage").then((module) => ({ default: module.ChatSessionPage })),
);
const ProjectsPage = lazy(() =>
  import("@/pages/projects/ProjectsPage").then((module) => ({ default: module.ProjectsPage })),
);
const ProjectSessionsPage = lazy(() =>
  import("@/pages/projects/ProjectSessionsPage").then((module) => ({ default: module.ProjectSessionsPage })),
);
const MemoryPage = lazy(() =>
  import("@/pages/memory/MemoryPage").then((module) => ({ default: module.MemoryPage })),
);
const WorkspaceDocsPage = lazy(() =>
  import("@/pages/workspace-docs/WorkspaceDocsPage").then((module) => ({ default: module.WorkspaceDocsPage })),
);
const FilesPage = lazy(() =>
  import("@/pages/files/FilesPage").then((module) => ({ default: module.FilesPage })),
);
const PluginsPage = lazy(() =>
  import("@/pages/plugins/PluginsPage").then((module) => ({ default: module.PluginsPage })),
);
const SettingsPage = lazy(() =>
  import("@/pages/settings/SettingsPage").then((module) => ({ default: module.SettingsPage })),
);
const AuthPage = lazy(() =>
  import("@/pages/auth/AuthPage").then((module) => ({ default: module.AuthPage })),
);

function RequireAuth({ children }: { children: ReactNode }) {
  const { connection } = useConnectionSettings();
  const location = useLocation();
  if (connection.authStatus === "loading" || connection.authStatus === "unknown") {
    return <AuthLoading />;
  }
  if (connection.authStatus !== "authenticated" && connection.authStatus !== "open") {
    const next = `${location.pathname}${location.search}${location.hash}`;
    return <Navigate replace to={`/auth/login?next=${encodeURIComponent(next)}`} />;
  }
  return <>{children}</>;
}

function AuthLoading() {
  const { t } = useTranslation("common");
  return <div className="flex h-screen items-center justify-center text-sm text-muted">{t("loading")}</div>;
}

function withSuspense(element: ReactNode) {
  function LoadingFallback() {
    const { t } = useTranslation("common");
    return <div className="p-8 text-sm text-muted">{t("loading")}</div>;
  }

  return <Suspense fallback={<LoadingFallback />}>{element}</Suspense>;
}

const isDesktopBuild = import.meta.env.VITE_TALOS_DESKTOP === "1";
const createRouter = isDesktopBuild ? createHashRouter : createBrowserRouter;

export const router = createRouter([
  {
    path: "/auth",
    element: <Navigate replace to="/auth/login" />,
  },
  {
    path: "/auth/login",
    element: withSuspense(<AuthPage />),
  },
  {
    path: "/auth/logout",
    element: withSuspense(<AuthPage />),
  },
  {
    path: "/",
    element: <RequireAuth>{<AppLayout />}</RequireAuth>,
    children: [
      {
        index: true,
        element: withSuspense(<ChatHomePage />),
      },
      {
        path: "sessions/:sessionId",
        element: withSuspense(<ChatSessionPage />),
      },
      {
        path: "projects",
        element: withSuspense(<ProjectsPage />),
      },
      {
        path: "projects/:projectId",
        element: withSuspense(<ProjectSessionsPage />),
      },
      {
        path: "memory",
        element: withSuspense(<MemoryPage />),
      },
      {
        path: "workspace-docs",
        element: withSuspense(<WorkspaceDocsPage />),
      },
      {
        path: "files",
        element: withSuspense(<FilesPage />),
      },
      {
        path: "plugins",
        element: withSuspense(<PluginsPage />),
      },
      {
        path: "settings",
        element: withSuspense(<SettingsPage />),
      },
      {
        path: "settings/:section",
        element: withSuspense(<SettingsPage />),
      },
    ],
  },
]);
