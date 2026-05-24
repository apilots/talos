import { useLocation, useNavigate } from "react-router-dom";

import {
  useCreateSessionMutation,
  useDeleteSessionMutation,
  useModelsQuery,
  useSessionsQuery,
} from "../hooks/useChatQueries";
import { logoutUser } from "@/features/auth/api";
import { useAppStore } from "@/shared/store/app-store";
import { getEffectiveSessionStatus } from "@/shared/types/domain";
import { Sidebar } from "@/widgets/sidebar/Sidebar";

export function SidebarContainer() {
  const location = useLocation();
  const navigate = useNavigate();
  const { data: models } = useModelsQuery();
  const createSession = useCreateSessionMutation();
  const deleteSession = useDeleteSessionMutation();
  const threadFilter = useAppStore((state) => state.threadFilter);
  const setThreadFilter = useAppStore((state) => state.setThreadFilter);
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const aliases = useAppStore((state) => state.sessionAliases);
  const statuses = useAppStore((state) => state.transientStatuses);
  const streams = useAppStore((state) => state.streams);
  const sessionsQuery = useSessionsQuery();

  const sessions = sessionsQuery.data ?? [];
  const threads = sessions.map((session) => {
    const sessionId = session.context.session_id;
    return {
      id: sessionId,
      label: aliases[sessionId] ?? session.title,
      status: getEffectiveSessionStatus(statuses[sessionId], streams[sessionId]),
      model: session.config.llm?.model ?? session.config.agent?.model ?? "unknown",
      lastActivityAt: session.state.last_activity,
      createdAt: session.state.created_at,
    };
  });

  async function handleLogout() {
    try {
      await logoutUser();
    } finally {
      setConnection({
        authToken: "",
        authStatus: "unauthenticated",
        authRole: "user",
        authUsername: "web-user",
        userId: "web-user",
      });
      navigate("/auth/login");
    }
  }

  return (
    <Sidebar
      connection={connection}
      currentPath={location.pathname}
      isHomePage={location.pathname === "/"}
      onDeleteThread={async (sessionId) => {
        await deleteSession.mutateAsync(sessionId);
        if (location.pathname === `/sessions/${sessionId}`) {
          navigate("/");
        }
      }}
      onCreateThread={async () => {
        const model = models?.[0]?.id;
        const session = await createSession.mutateAsync({
          model,
        });
        navigate(`/sessions/${session.context.session_id}`);
      }}
      onCycleThreadFilter={() =>
        setThreadFilter(
          threadFilter === "all"
            ? "running"
            : "all",
        )
      }
      onSelectThread={(sessionId) => navigate(`/sessions/${sessionId}`)}
      threadCount={sessions.length}
      threadFilter={threadFilter}
      threads={threads}
      onLogout={handleLogout}
    />
  );
}
