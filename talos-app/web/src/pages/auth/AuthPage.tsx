import { type FormEvent, useEffect, useMemo, useState } from "react";
import { LogIn, UserPlus } from "lucide-react";
import { useNavigate } from "react-router-dom";

import { getCloudAuthUrl, getCurrentUser, storeDesktopCloudSession } from "@/features/auth";
import { useConnectionSettings } from "@/features/connection";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

export function AuthPage() {
  const navigate = useNavigate();
  const { connection, setConnection } = useConnectionSettings();
  const [error, setError] = useState("");
  const [isCompleting, setIsCompleting] = useState(false);
  const [cloudAuthUrl, setCloudAuthUrl] = useState("");
  const [isLoadingConfig, setIsLoadingConfig] = useState(true);

  const redirectUri = useMemo(() => `${window.location.origin}/auth/login`, []);

  useEffect(() => {
    if (connection.authStatus === "authenticated" || connection.authStatus === "open") {
      navigate("/");
    }
  }, [connection.authStatus, navigate]);

  useEffect(() => {
    let canceled = false;
    setIsLoadingConfig(true);
    void getCloudAuthUrl()
      .then((url) => {
        if (!canceled) {
          setCloudAuthUrl(url);
          setError("");
        }
      })
      .catch(() => {
        if (!canceled) {
          setError("Cloud auth URL is not configured by the Talos server.");
        }
      })
      .finally(() => {
        if (!canceled) {
          setIsLoadingConfig(false);
        }
      });
    return () => {
      canceled = true;
    };
  }, []);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const accessToken = params.get("access_token");
    if (!accessToken || isCompleting || !cloudAuthUrl) {
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
        void storeDesktopCloudSession(accessToken, cloudAuthUrl, user);
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
  }, [cloudAuthUrl, isCompleting, navigate, setConnection]);

  function openCloudAuth(path: string) {
    if (!cloudAuthUrl) {
      setError("Cloud auth URL is not configured by the Talos server.");
      return;
    }
    const url = new URL(path, cloudAuthUrl);
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
          <Button className="w-full" disabled={isCompleting || isLoadingConfig || !cloudAuthUrl} type="submit">
            <LogIn size={16} />
            Continue with Cloud Login
          </Button>
          <Button
            className="w-full"
            disabled={isCompleting || isLoadingConfig || !cloudAuthUrl}
            onClick={() => openCloudAuth("/auth/register")}
            type="button"
            variant="secondary"
          >
            <UserPlus size={16} />
            Create Cloud Account
          </Button>
        </form>

        {error ? <p className="mt-4 text-sm text-danger">{error}</p> : null}
      </Panel>
    </div>
  );
}
