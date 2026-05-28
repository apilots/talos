import { useEffect, useRef } from "react";

import {
  clearDesktopCloudSession,
  getCurrentUser,
  getCurrentUserWithToken,
  getGatewayHealth,
  readDesktopCloudSession,
  storeDesktopCloudSession,
} from "../api";
import { useConnectionSettings } from "@/features/connection";

export function useAuthBootstrap() {
  const { connection, setConnection } = useConnectionSettings();
  const attemptedKeyRef = useRef<string | null>(null);
  const inflightKeyRef = useRef<string | null>(null);

  useEffect(() => {
    const attemptKey = `${connection.serverUrl}::${connection.authToken || "__anonymous__"}`;
    if (attemptedKeyRef.current === attemptKey || inflightKeyRef.current === attemptKey) {
      return;
    }
    inflightKeyRef.current = attemptKey;

    let canceled = false;
    setConnection({ authStatus: "loading" });

    void getGatewayHealth()
      .then(async (health) => {
        if (canceled) {
          return;
        }
        if (!health.auth_enabled) {
          attemptedKeyRef.current = attemptKey;
          setConnection({
            authStatus: "open",
            authToken: "",
            authUsername: "Anonymous",
            authRole: "user",
            userId: "anonymous",
          });
          return;
        }
        let restoredToken = "";
        let restoredCloudApiBaseUrl = "";
        if (!connection.authToken) {
          const session = await readDesktopCloudSession();
          if (session?.access_token) {
            restoredToken = session.access_token;
            restoredCloudApiBaseUrl = session.cloud_api_base_url;
          }
        }
        const user = restoredToken
          ? await getCurrentUserWithToken(restoredToken)
          : await getCurrentUser();
        if (canceled) {
          return;
        }
        if (restoredToken) {
          void storeDesktopCloudSession(restoredToken, restoredCloudApiBaseUrl, user);
        }
        attemptedKeyRef.current = `${connection.serverUrl}::${restoredToken || connection.authToken || "__anonymous__"}`;
        setConnection({
          ...(restoredToken ? { authToken: restoredToken } : {}),
          authStatus: "authenticated",
          authUsername: user.username,
          authRole: user.role,
          userId: user.user_id,
        });
      })
      .catch(() => {
        if (canceled) {
          return;
        }
        void clearDesktopCloudSession();
        attemptedKeyRef.current = attemptKey;
        setConnection({
          authStatus: "unauthenticated",
          authToken: "",
          authUsername: "Anonymous",
          authRole: "user",
          userId: "anonymous",
        });
      })
      .finally(() => {
        if (inflightKeyRef.current === attemptKey) {
          inflightKeyRef.current = null;
        }
      });

    return () => {
      canceled = true;
      if (inflightKeyRef.current === attemptKey) {
        inflightKeyRef.current = null;
      }
    };
  }, [connection.authStatus, connection.authToken, connection.serverUrl, setConnection]);
}
