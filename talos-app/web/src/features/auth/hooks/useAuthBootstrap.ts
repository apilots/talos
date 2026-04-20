import { useEffect } from "react";

import { getCurrentUser } from "../api";
import { useConnectionSettings } from "@/features/connection";

export function useAuthBootstrap() {
  const { connection, setConnection } = useConnectionSettings();

  useEffect(() => {
    const hadValidToken = Boolean(connection.authToken);

    if (!hadValidToken) {
      setConnection({
        authStatus: "unauthenticated",
        authUsername: "web-user",
        authRole: "user",
        userId: "web-user",
      });
      return;
    }

    if (connection.authStatus === "authenticated") {
      return;
    }

    let canceled = false;
    setConnection({ authStatus: "loading" });

    void getCurrentUser()
      .then((user) => {
        if (canceled) {
          return;
        }
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
        setConnection({
          authStatus: "unauthenticated",
          authToken: "",
          authUsername: "web-user",
          authRole: "user",
          userId: "web-user",
        });
      });

    return () => {
      canceled = true;
    };
  }, [connection.authStatus, connection.authToken, setConnection]);
}
