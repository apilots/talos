import { useEffect, useRef } from "react";

import { getCurrentUser, getGatewayHealth } from "../api";
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
        const user = await getCurrentUser();
        if (canceled) {
          return;
        }
        attemptedKeyRef.current = attemptKey;
        setConnection({
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
