import { lazy, Suspense, type ReactNode } from "react";
import { createBrowserRouter } from "react-router-dom";
import { useTranslation } from "react-i18next";

import { AppLayout } from "@/app/layouts/AppLayout";

const ChatHomePage = lazy(() =>
  import("@/pages/chat/ChatHomePage").then((module) => ({ default: module.ChatHomePage })),
);
const ChatSessionPage = lazy(() =>
  import("@/pages/chat/ChatSessionPage").then((module) => ({ default: module.ChatSessionPage })),
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

function withSuspense(element: ReactNode) {
  function LoadingFallback() {
    const { t } = useTranslation("common");
    return <div className="p-8 text-sm text-muted">{t("loading")}</div>;
  }

  return <Suspense fallback={<LoadingFallback />}>{element}</Suspense>;
}

export const router = createBrowserRouter([
  {
    path: "/",
    element: <AppLayout />,
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
        element: withSuspense(<AdminPage />),
      },
    ],
  },
]);
