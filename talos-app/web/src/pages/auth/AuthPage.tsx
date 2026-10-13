import { type FormEvent, useEffect, useMemo, useState } from "react";
import { LogIn } from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";

import {
  closePreparedExternalWindow,
  getCloudLoginStatus,
  prepareExternalAuthWindow,
  redirectToExternalUrl,
  startCloudLogin,
  storeWebAuthToken,
} from "@/features/auth";
import { useConnectionSettings } from "@/features/connection";
import { DesktopConnectionControl } from "@/features/computers";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";
import { accountTypeFromWire } from "@/shared/types/account";

export function AuthPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const { connection, setConnection } = useConnectionSettings();
  const [error, setError] = useState("");
  const [isCompleting, setIsCompleting] = useState(false);
  const [loginId, setLoginId] = useState("");
  const [isLoadingConfig, setIsLoadingConfig] = useState(true);

  const params = useMemo(() => new URLSearchParams(location.search), [location.search]);
  const nextPath = sanitizeNextPath(params.get("next"));
  const isLoggedOut = location.pathname === "/auth/logout" || connection.authStatus === "logged_out";
  const logoSrc = `${import.meta.env.BASE_URL}talos-logo.svg`;

  useEffect(() => {
    if (connection.authStatus === "authenticated" || connection.authStatus === "open") {
      navigate(nextPath, { replace: true });
    }
  }, [connection.authStatus, navigate, nextPath]);

  useEffect(() => {
    setIsLoadingConfig(false);
  }, []);

  useEffect(() => {
    if (!loginId || isCompleting) {
      return;
    }

    let canceled = false;
    let timeout: number | undefined;

    async function poll() {
      try {
        const response = await getCloudLoginStatus(loginId);
        if (canceled) return;
        if (response.status === "pending") {
          timeout = window.setTimeout(poll, response.interval_ms);
          return;
        }
        if (response.status === "authenticated") {
          if (canceled) return;
          storeWebAuthToken(response.access_token);
          setConnection({
            authToken: response.access_token,
            authStatus: "authenticated",
            authUsername: response.user.username,
            authRole: response.user.role,
            accountType: accountTypeFromWire(response.user.type),
            userId: response.user.user_id,
          });
          navigate(nextPath, { replace: true });
          return;
        }
        setConnection({
          authToken: "",
          authStatus: "unauthenticated",
          authUsername: "Anonymous",
          authRole: "user",
          accountType: "Free",
          userId: "anonymous",
        });
        setError(response.status === "expired" ? "Cloud login expired." : "Cloud login was denied.");
      } catch {
        if (canceled) return;
        setConnection({
          authToken: "",
          authStatus: "unauthenticated",
          authUsername: "Anonymous",
          authRole: "user",
          accountType: "Free",
          userId: "anonymous",
        });
        setError("Cloud login status could not be verified.");
      }
    }

    void poll();
    return () => {
      canceled = true;
      if (timeout !== undefined) window.clearTimeout(timeout);
    };
  }, [isCompleting, loginId, navigate, nextPath, setConnection]);

  async function openCloudAuth() {
    const authWindow = prepareExternalAuthWindow();
    setIsCompleting(true);
    setError("");
    storeWebAuthToken("");
    setConnection({ authToken: "", authStatus: "loading" });
    try {
      const response = await startCloudLogin();
      setLoginId(response.login_id);
      redirectToExternalUrl(response.login_url, authWindow);
    } catch {
      closePreparedExternalWindow(authWindow);
      setError("Cloud login could not be started.");
      setConnection({ authStatus: "unauthenticated" });
    } finally {
      setIsCompleting(false);
    }
  }

  function handleLogin(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void openCloudAuth();
  }

  return (
    <div className="flex h-screen items-center justify-center bg-background p-6 text-ink">
      <Panel className="w-full max-w-[360px] rounded-[14px] border-border bg-card p-6 shadow-panel backdrop-blur-none">
        <div className="text-center">
          <img
            alt="Talos"
            className="mx-auto size-14 rounded-[15px] shadow-[0_8px_22px_rgba(27,24,19,0.08)]"
            src={logoSrc}
          />
          <h1 className="mt-5 text-[22px] font-semibold leading-7 text-ink">
            {isLoggedOut ? "Signed out of Talos" : "Sign in to Talos"}
          </h1>
          <p className="mx-auto mt-2 max-w-[280px] text-[13px] leading-5 text-muted">
            {isLoggedOut
              ? "Your local session has been cleared."
              : "Use your Talos Cloud account to continue."}
          </p>
        </div>

        <form className="mt-7 grid gap-3" onSubmit={handleLogin}>
          <Button
            className="h-10 w-full rounded-[10px] border border-border bg-card text-[13px] font-medium text-ink shadow-[0_1px_0_rgba(27,24,19,0.04)] hover:border-muted/40 hover:bg-surface disabled:bg-surface"
            disabled={isCompleting || isLoadingConfig}
            type="submit"
            variant="secondary"
          >
            <LogIn size={16} />
            Continue with Talos Cloud
          </Button>
        </form>

        {!isLoggedOut ? (
          <button
            className="mt-4 w-full text-center text-[12px] font-medium text-muted transition hover:text-ink disabled:cursor-not-allowed disabled:text-muted/60"
            disabled={isCompleting || isLoadingConfig}
            onClick={() => void openCloudAuth()}
            type="button"
          >
            <span>Need an account?</span>{" "}
            <span className="text-ink transition hover:text-ink">
              Create one in Talos Cloud
            </span>
          </button>
        ) : null}

        <DesktopConnectionControl />
        {error ? <p className="mt-4 text-center text-[13px] leading-5 text-danger">{error}</p> : null}
      </Panel>
    </div>
  );
}

function sanitizeNextPath(value: string | null): string {
  if (!value || !value.startsWith("/") || value.startsWith("//") || value.startsWith("/auth/")) {
    return "/";
  }
  return value;
}
