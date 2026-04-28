import { type FormEvent, useEffect, useState } from "react";
import { LogIn, User, UserPlus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { loginUser, registerUser } from "@/features/auth";
import { useConnectionSettings } from "@/features/connection";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

type AuthMode = "login" | "register";

export function AuthPage() {
  const { t } = useTranslation("auth");
  const navigate = useNavigate();
  const { connection, setConnection } = useConnectionSettings();
  const [mode, setMode] = useState<AuthMode>("login");
  const [username, setUsername] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (connection.authStatus === "authenticated" || connection.authStatus === "open") {
      navigate("/");
    }
  }, [connection.authStatus, navigate]);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmedUsername = username.trim();
    const trimmedEmail = email.trim();
    const trimmedPassword = password.trim();

    if (!trimmedUsername || !trimmedPassword || (mode === "register" && !trimmedEmail)) {
      setError(t("requiredFields"));
      return;
    }

    setIsSubmitting(true);
    setError("");
    try {
      const authResponse =
        mode === "login"
          ? await loginUser({ username: trimmedUsername, password: trimmedPassword })
          : await registerUser({
              username: trimmedUsername,
              email: trimmedEmail,
              password: trimmedPassword,
            });

      setConnection({
        authToken: authResponse.access_token,
        authStatus: "authenticated",
        authUsername: authResponse.user.username,
        authRole: authResponse.user.role,
        userId: authResponse.user.user_id,
      });
      navigate("/");
    } catch {
      setError(t("submitError"));
    } finally {
      setIsSubmitting(false);
    }
  }

  function clearError() {
    setError("");
  }

  function switchMode(nextMode: AuthMode) {
    setMode(nextMode);
    clearError();
  }

  return (
    <div className="flex h-screen items-center justify-center bg-[radial-gradient(circle_at_top,rgba(255,171,92,0.14),transparent_38%),linear-gradient(180deg,rgba(255,248,239,0.96),rgba(248,242,233,0.92))] p-6">
      <Panel className="w-full max-w-[440px] rounded-[1.6rem] border-border/70 bg-card/94 p-7 shadow-[0_24px_70px_rgba(95,84,64,0.16)] backdrop-blur">
        <div className="space-y-2">
          <div className="text-[11px] uppercase tracking-[0.24em] text-muted">Talos</div>
          <h1 className="text-2xl font-semibold text-ink">{t("title")}</h1>
          <p className="max-w-[28rem] text-sm leading-6 text-muted">{t("subtitle")}</p>
        </div>

        <div className="mt-6 flex items-center gap-6 border-b border-border/80">
          <button
            className={`relative -mb-px border-b-2 px-0 pb-3 text-sm font-medium transition ${
              mode === "login"
                ? "border-brand text-ink"
                : "border-transparent text-muted hover:border-border/90 hover:text-ink"
            }`}
            onClick={() => switchMode("login")}
            type="button"
          >
            <span className="inline-flex items-center gap-2">
              <LogIn size={16} />
              {t("modeLogin")}
            </span>
          </button>
          <button
            className={`relative -mb-px border-b-2 px-0 pb-3 text-sm font-medium transition ${
              mode === "register"
                ? "border-brand text-ink"
                : "border-transparent text-muted hover:border-border/90 hover:text-ink"
            }`}
            onClick={() => switchMode("register")}
            type="button"
          >
            <span className="inline-flex items-center gap-2">
              <UserPlus size={16} />
              {t("modeRegister")}
            </span>
          </button>
        </div>

        <form className="mt-5 space-y-4" onSubmit={handleSubmit}>
          <div>
            <label className="mb-2 block text-xs uppercase tracking-[0.18em] text-muted" htmlFor="auth-username">
              {t("username")}
            </label>
            <div className="relative">
              <User className="absolute left-3 top-1/2 -translate-y-1/2 text-muted" size={16} />
              <Input
                autoComplete="username"
                className="pl-10"
                id="auth-username"
                onChange={(event) => {
                  setUsername(event.target.value);
                  clearError();
                }}
                value={username}
              />
            </div>
          </div>

          {mode === "register" ? (
            <div>
              <label className="mb-2 block text-xs uppercase tracking-[0.18em] text-muted" htmlFor="auth-email">
                {t("email")}
              </label>
              <Input
                autoComplete="email"
                id="auth-email"
                onChange={(event) => {
                  setEmail(event.target.value);
                  clearError();
                }}
                type="email"
                value={email}
              />
            </div>
          ) : null}

          <div>
            <label className="mb-2 block text-xs uppercase tracking-[0.18em] text-muted" htmlFor="auth-password">
              {t("password")}
            </label>
            <Input
              autoComplete={mode === "login" ? "current-password" : "new-password"}
              id="auth-password"
              onChange={(event) => {
                setPassword(event.target.value);
                clearError();
              }}
              type="password"
              value={password}
            />
          </div>

          {error ? <p className="text-sm text-danger">{error}</p> : null}

          <Button
            className="w-full"
            disabled={
              isSubmitting || !username.trim() || !password.trim() || (mode === "register" && !email.trim())
            }
            type="submit"
          >
            {isSubmitting ? t("submitting") : mode === "login" ? t("login") : t("register")}
          </Button>
        </form>
      </Panel>
    </div>
  );
}
