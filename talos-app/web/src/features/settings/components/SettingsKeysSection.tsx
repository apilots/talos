import { useState } from "react";
import { KeyRound, Plus, Trash2, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { formatDateTime, formatRelativeTime } from "@/shared/lib/time";
import { Panel } from "@/shared/ui/Panel";
import type { CreateUserTokenResult } from "../api";
import { useUserTokens } from "../hooks/useUserTokens";
import { SectionHeader } from "./SectionPrimitives";

function parseErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  return "Request failed.";
}

export function SettingsKeysSection() {
  const { t } = useTranslation(["settings", "common"]);
  const { tokensQuery, createToken, revokeToken, createMutation, revokeMutation } = useUserTokens();
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [tokenName, setTokenName] = useState("");
  const [createdToken, setCreatedToken] = useState<CreateUserTokenResult | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const [actionError, setActionError] = useState("");

  const tokens = tokensQuery.data ?? [];
  const createError = createMutation.error ? parseErrorMessage(createMutation.error) : "";

  function openCreateDialog() {
    setTokenName("");
    setCreatedToken(null);
    setCopyState("idle");
    setActionError("");
    setIsCreateDialogOpen(true);
  }

  function closeCreateDialog() {
    setIsCreateDialogOpen(false);
    setTokenName("");
    setCreatedToken(null);
    setCopyState("idle");
  }

  async function handleCreateToken() {
    const normalizedName = tokenName.trim();
    if (!normalizedName) {
      return;
    }
    const result = await createToken(normalizedName);
    setCreatedToken(result);
    setTokenName("");
    setCopyState("idle");
  }

  async function handleCopyToken() {
    if (!createdToken?.raw_token || !navigator.clipboard) {
      setCopyState("failed");
      return;
    }
    try {
      await navigator.clipboard.writeText(createdToken.raw_token);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  }

  async function handleRevokeToken(tokenId: string) {
    setActionError("");
    if (!window.confirm(t("revokeTokenConfirm"))) {
      return;
    }
    try {
      await revokeToken(tokenId);
    } catch (error) {
      setActionError(parseErrorMessage(error));
    }
  }

  return (
    <>
      <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
        <SectionHeader
          icon={<KeyRound size={18} />}
          subtitle={t("apiKeysDetail")}
          title={t("apiKeysTitle")}
        />

        <div className="mt-5 flex flex-wrap items-center justify-between gap-3 rounded-[1.2rem] border border-border/80 bg-background/45 p-4">
          <div>
            <div className="text-sm font-medium text-ink">{t("tokenStatsTitle")}</div>
            <div className="mt-1 text-sm text-muted">{t("tokenStatsDetail", { count: tokens.length })}</div>
          </div>
          <Button onClick={openCreateDialog} type="button">
            <Plus size={16} />
            {t("createToken")}
          </Button>
        </div>

        {actionError ? (
          <div className="mt-4 rounded-[1rem] border border-danger/20 bg-danger/8 px-4 py-3 text-sm text-danger">
            {t("tokenActionError", { message: actionError })}
          </div>
        ) : null}

        {tokensQuery.isLoading ? (
          <div className="mt-6 py-8 text-sm text-muted">{t("common:loading")}</div>
        ) : tokensQuery.isError ? (
          <div className="mt-6 rounded-[1rem] border border-danger/20 bg-danger/8 px-4 py-3 text-sm text-danger">
            {parseErrorMessage(tokensQuery.error)}
          </div>
        ) : tokens.length === 0 ? (
          <div className="mt-6 rounded-[1.2rem] border border-dashed border-border/80 bg-background/30 px-5 py-8 text-sm text-muted">
            <div className="font-medium text-ink">{t("noTokensTitle")}</div>
            <div className="mt-1">{t("noTokensDetail")}</div>
          </div>
        ) : (
          <div className="mt-6 space-y-3">
            {tokens.map((token) => {
              const isRevoking = revokeMutation.isPending && revokeMutation.variables === token.token_id;
              return (
                <div
                  className="flex flex-col gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 p-4 md:flex-row md:items-center md:justify-between"
                  key={token.token_id}
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <div className="truncate font-medium text-ink">{token.name}</div>
                      <Badge className="font-mono normal-case tracking-normal" tone="default">
                        {token.prefix}
                      </Badge>
                    </div>
                    <div className="mt-2 flex flex-wrap gap-x-5 gap-y-2 text-xs text-muted">
                      <span>{t("tokenCreatedAt", { date: formatDateTime(token.created_at) })}</span>
                      <span>
                        {token.last_used_at
                          ? t("tokenLastUsed", { date: formatRelativeTime(token.last_used_at) })
                          : t("tokenNeverUsed")}
                      </span>
                      {token.expires_at ? (
                        <span>{t("tokenExpiresAt", { date: formatDateTime(token.expires_at) })}</span>
                      ) : null}
                    </div>
                  </div>

                  <Button
                    disabled={isRevoking}
                    onClick={() => void handleRevokeToken(token.token_id)}
                    type="button"
                    variant="danger"
                  >
                    <Trash2 size={16} />
                    {isRevoking ? t("revokingToken") : t("revokeToken")}
                  </Button>
                </div>
              );
            })}
          </div>
        )}
      </Panel>

      {isCreateDialogOpen ? (
        <div className="fixed inset-0 z-40 flex items-center justify-center px-4 py-6">
          <div className="absolute inset-0 bg-black/55" onClick={closeCreateDialog} role="presentation" />
          <div className="relative z-10 w-full max-w-xl rounded-[1.6rem] border border-border/80 bg-card p-6 shadow-panel">
            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="text-sm font-medium text-ink">
                  {createdToken ? t("tokenCreatedTitle") : t("createTokenTitle")}
                </div>
                <div className="mt-1 text-sm text-muted">
                  {createdToken ? t("tokenCreatedDetail") : t("createTokenDetail")}
                </div>
              </div>
              <button
                className="inline-flex h-9 w-9 items-center justify-center rounded-full text-muted transition hover:bg-background hover:text-ink"
                onClick={closeCreateDialog}
                type="button"
              >
                <X size={18} />
              </button>
            </div>

            {createdToken ? (
              <div className="mt-5 space-y-4">
                <div className="rounded-[1rem] border border-warning/25 bg-warning/8 px-4 py-3 text-sm text-warning">
                  {t("tokenRevealWarning")}
                </div>

                <div className="rounded-[1rem] border border-border/80 bg-background/60 px-4 py-4">
                  <div className="text-xs uppercase tracking-[0.16em] text-muted">{t("tokenValueLabel")}</div>
                  <div className="mt-2 break-all font-mono text-sm text-ink">{createdToken.raw_token}</div>
                </div>

                <div className="text-sm text-muted">{t("tokenPrefixHint", { prefix: createdToken.prefix })}</div>

                <div className="flex flex-wrap justify-end gap-2">
                  <Button onClick={() => void handleCopyToken()} type="button" variant="secondary">
                    {copyState === "copied"
                      ? t("tokenCopied")
                      : copyState === "failed"
                        ? t("tokenCopyFailed")
                        : t("tokenCopy")}
                  </Button>
                  <Button onClick={closeCreateDialog} type="button">
                    {t("common:close")}
                  </Button>
                </div>
              </div>
            ) : (
              <div className="mt-5 space-y-4">
                <div>
                  <label className="mb-2 block text-sm text-muted" htmlFor="token-name-input">
                    {t("tokenNameLabel")}
                  </label>
                  <Input
                    id="token-name-input"
                    onChange={(event) => setTokenName(event.target.value)}
                    placeholder={t("tokenNamePlaceholder")}
                    value={tokenName}
                  />
                </div>

                {createError ? (
                  <div className="rounded-[1rem] border border-danger/20 bg-danger/8 px-4 py-3 text-sm text-danger">
                    {createError}
                  </div>
                ) : null}

                <div className="flex flex-wrap justify-end gap-2">
                  <Button onClick={closeCreateDialog} type="button" variant="ghost">
                    {t("common:cancel")}
                  </Button>
                  <Button
                    disabled={createMutation.isPending || tokenName.trim().length === 0}
                    onClick={() => void handleCreateToken()}
                    type="button"
                  >
                    {createMutation.isPending ? t("creatingToken") : t("createTokenSubmit")}
                  </Button>
                </div>
              </div>
            )}
          </div>
        </div>
      ) : null}
    </>
  );
}
