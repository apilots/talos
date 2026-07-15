import { useEffect, useRef } from "react";

import {
  clearDesktopCloudSession,
  getCurrentUser,
  getCurrentUserWithToken,
  getGatewayHealth,
  readDesktopCloudSession,
} from "../api";
import { clearWebAuthToken, readWebAuthToken } from "../web-auth-session";
import { resolveServerBaseUrl } from "@/shared/api/server-base-url";
import { useConnectionSettings } from "@/features/connection";
import { accountTypeFromWire } from "@/shared/types/account";

export function useAuthBootstrap() {
  const { connection, setConnection } = useConnectionSettings();
  const attemptedKeyRef = useRef<string | null>(null);
  const inflightKeyRef = useRef<string | null>(null);

  useEffect(() => {
    if (connection.authStatus === "logged_out") {
      return;
    }

    const serverBaseUrl = resolveServerBaseUrl();
    const attemptKey = `${serverBaseUrl}::${connection.authToken || "__anonymous__"}`;
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
          clearWebAuthToken();
          attemptedKeyRef.current = attemptKey;
          setConnection({
            authStatus: "open",
            authToken: "",
            authUsername: "Anonymous",
            authRole: "user",
            accountType: "Free",
            userId: "anonymous",
          });
          return;
        }
        let restoredToken = "";
        if (!connection.authToken) {
          const session = await readDesktopCloudSession();
          if (session?.access_token) {
            restoredToken = session.access_token;
          } else {
            restoredToken = readWebAuthToken() ?? "";
          }
        }
        const authToken = restoredToken || connection.authToken;
        const user = authToken ? await getCurrentUserWithToken(authToken) : await getCurrentUser();
        if (canceled) {
          return;
        }
        attemptedKeyRef.current = `${serverBaseUrl}::${restoredToken || connection.authToken || "__anonymous__"}`;
        setConnection({
          ...(restoredToken ? { authToken: restoredToken } : {}),
          authStatus: "authenticated",
          authUsername: user.username,
          authRole: user.role,
          accountType: accountTypeFromWire(user.type),
          userId: user.user_id,
        });
      })
      .catch(() => {
        if (canceled) {
          return;
        }
        void clearDesktopCloudSession();
        clearWebAuthToken();
        attemptedKeyRef.current = attemptKey;
        setConnection({
          authStatus: "unauthenticated",
          authToken: "",
          authUsername: "Anonymous",
          authRole: "user",
          accountType: "Free",
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
  }, [connection.authStatus, connection.authToken, setConnection]);
}
