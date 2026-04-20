import { type ReactNode, lazy, Suspense } from "react";
import { Navigate, createBrowserRouter } from "react-router-dom";
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
const FilesPage = lazy(() =>
  import("@/pages/files/FilesPage").then((module) => ({ default: module.FilesPage })),
);
const SettingsPage = lazy(() =>
  import("@/pages/settings/SettingsPage").then((module) => ({ default: module.SettingsPage })),
);
const AdminPage = lazy(() =>
  import("@/pages/admin/AdminPage").then((module) => ({ default: module.AdminPage })),
);
const AuthPage = lazy(() =>
  import("@/pages/auth/AuthPage").then((module) => ({ default: module.AuthPage })),
);

function RequireAuth({ children }: { children: ReactNode }) {
  const { connection } = useConnectionSettings();
  if (connection.authStatus === "loading" || connection.authStatus === "unknown") {
    return <AuthLoading />;
  }
  if (connection.authStatus !== "authenticated") {
    return <Navigate replace to="/auth/login" />;
  }
  return <>{children}</>;
}

function RequireAdmin({ children }: { children: ReactNode }) {
  const { connection } = useConnectionSettings();
  if (connection.authStatus === "loading" || connection.authStatus === "unknown") {
    return <AuthLoading />;
  }
  if (connection.authStatus !== "authenticated" || connection.authRole !== "admin") {
    return <Navigate replace to="/" />;
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

export const router = createBrowserRouter([
  {
    path: "/auth/login",
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
        path: "files",
        element: withSuspense(<FilesPage />),
      },
      {
        path: "settings",
        element: withSuspense(<SettingsPage />),
      },
      {
        path: "admin",
        element: withSuspense(
          <RequireAdmin>
            <AdminPage />
          </RequireAdmin>,
        ),
      },
    ],
  },
]);
