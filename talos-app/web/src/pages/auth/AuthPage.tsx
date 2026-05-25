import { type FormEvent, useEffect, useMemo, useState } from "react";
import { ExternalLink, LogIn, UserPlus } from "lucide-react";
import { useNavigate } from "react-router-dom";

import { getCurrentUser } from "@/features/auth";
import { useConnectionSettings } from "@/features/connection";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

const defaultCloudAuthUrl = import.meta.env.VITE_TALOS_CLOUD_AUTH_URL ?? "http://127.0.0.1:8787";

export function AuthPage() {
  const navigate = useNavigate();
  const { connection, setConnection } = useConnectionSettings();
  const [error, setError] = useState("");
  const [isCompleting, setIsCompleting] = useState(false);

  const redirectUri = useMemo(() => `${window.location.origin}/auth/login`, []);

  useEffect(() => {
    if (connection.authStatus === "authenticated" || connection.authStatus === "open") {
      navigate("/");
    }
  }, [connection.authStatus, navigate]);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const accessToken = params.get("access_token");
    if (!accessToken || isCompleting) {
      return;
    }

    setIsCompleting(true);
    setError("");
    setConnection({ authToken: accessToken, authStatus: "loading" });
    void getCurrentUser()
      .then((user) => {
        setConnection({
          authToken: accessToken,
          authStatus: "authenticated",
          authUsername: user.username,
          authRole: user.role,
          userId: user.user_id,
        });
        navigate("/", { replace: true });
      })
      .catch(() => {
        setConnection({
          authToken: "",
          authStatus: "unauthenticated",
          authUsername: "Anonymous",
          authRole: "user",
          userId: "anonymous",
        });
        setError("Cloud login callback could not be verified.");
      })
      .finally(() => setIsCompleting(false));
  }, [isCompleting, navigate, setConnection]);

  function openCloudAuth(path: string) {
    const url = new URL(path, defaultCloudAuthUrl);
    url.searchParams.set("redirect_uri", redirectUri);
    window.location.href = url.toString();
  }

  function handleLogin(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    openCloudAuth("/auth/login");
  }

  return (
    <div className="flex h-screen items-center justify-center bg-[linear-gradient(180deg,#f6f2ea,#ebe3d5)] p-6">
      <Panel className="w-full max-w-[440px] rounded-lg border-border/80 bg-card p-7 shadow-[0_24px_70px_rgba(68,58,39,0.15)]">
        <div className="space-y-2">
          <div className="text-[11px] uppercase tracking-[0.18em] text-muted">Talos Cloud</div>
          <h1 className="text-2xl font-semibold text-ink">Sign in to Talos</h1>
          <p className="text-sm leading-6 text-muted">
            Account registration and login are handled by Talos Cloud. Local Talos only consumes the verified cloud identity.
          </p>
        </div>

        <form className="mt-6 grid gap-3" onSubmit={handleLogin}>
          <Button className="w-full" disabled={isCompleting} type="submit">
            <LogIn size={16} />
            Continue with Cloud Login
          </Button>
          <Button
            className="w-full"
            disabled={isCompleting}
            onClick={() => openCloudAuth("/auth/register")}
            type="button"
            variant="secondary"
          >
            <UserPlus size={16} />
            Create Cloud Account
          </Button>
          <Button
            className="w-full"
            disabled={isCompleting}
            onClick={() => openCloudAuth("/auth/google")}
            type="button"
            variant="ghost"
          >
            <ExternalLink size={16} />
            Continue with Google
          </Button>
        </form>

        {error ? <p className="mt-4 text-sm text-danger">{error}</p> : null}
      </Panel>
    </div>
  );
}
