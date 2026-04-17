import { useEffect, useMemo, useState } from "react";
import { CheckCircle2, ServerCog } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { WorkspaceDocDetail, WorkspaceDocKind } from "@talos/sdk";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { getTalosClient } from "@/shared/api/client";
import { Button } from "@/shared/ui/Button";
import { Input, Textarea } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { FieldBlock, SectionHeader, ToggleRow } from "./SectionPrimitives";

const DOC_KINDS: WorkspaceDocKind[] = ["soul", "identity", "user", "heartbeat", "memory"];

function emptyDoc(kind: WorkspaceDocKind): WorkspaceDocDetail {
  return {
    kind,
    path: `${kind.toUpperCase()}.md`,
    exists: false,
    content: "",
    updated_at: null,
    version_token: null,
  };
}

export function SettingsServerSection({
  connection,
  setConnection,
  draft,
  setDraft,
  models,
  setLocale,
  setSafeFileReads,
  setSafeFileWrites,
  setTerminalApproval,
  safeFileReads,
  safeFileWrites,
  terminalApproval,
}: Pick<
  SettingsPageState,
  | "connection"
  | "setConnection"
  | "draft"
  | "setDraft"
  | "models"
  | "setLocale"
  | "setSafeFileReads"
  | "setSafeFileWrites"
  | "setTerminalApproval"
  | "safeFileReads"
  | "safeFileWrites"
  | "terminalApproval"
>) {
  const { t } = useTranslation(["settings", "common"]);
  const [selectedDocKind, setSelectedDocKind] = useState<WorkspaceDocKind>("memory");
  const [docDetail, setDocDetail] = useState<WorkspaceDocDetail>(emptyDoc("memory"));
  const [docDraft, setDocDraft] = useState("");
  const [docBusy, setDocBusy] = useState(false);
  const [docError, setDocError] = useState<string | null>(null);

  const docLabelMap = useMemo<Record<WorkspaceDocKind, string>>(
    () => ({
      soul: "SOUL.md",
      identity: "IDENTITY.md",
      user: "USER.md",
      heartbeat: "HEARTBEAT.md",
      memory: "MEMORY.md",
    }),
    [],
  );

  useEffect(() => {
    let cancelled = false;
    setDocBusy(true);
    setDocError(null);

    void getTalosClient()
      .workspaceDocs.get(selectedDocKind)
      .then((detail) => {
        if (cancelled) {
          return;
        }
        setDocDetail(detail);
        setDocDraft(detail.content);
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        setDocError(error instanceof Error ? error.message : "Failed to load workspace doc");
      })
      .finally(() => {
        if (!cancelled) {
          setDocBusy(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [selectedDocKind]);

  async function saveDoc() {
    setDocBusy(true);
    setDocError(null);
    try {
      const detail = await getTalosClient().workspaceDocs.put(selectedDocKind, {
        content: docDraft,
        base_version: docDetail.version_token
          ? { mode: "match", version_token: docDetail.version_token }
          : { mode: "must_not_exist" },
      });
      setDocDetail(detail);
      setDocDraft(detail.content);
    } catch (error) {
      setDocError(error instanceof Error ? error.message : "Failed to save workspace doc");
    } finally {
      setDocBusy(false);
    }
  }

  return (
    <div className="space-y-5">
      <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/88 p-6">
        <SectionHeader
          icon={<ServerCog size={18} />}
          subtitle={t("settings:serverDetail")}
          title={t("settings:serverTitle")}
        />
        <form
          className="mt-6 space-y-5"
          onSubmit={(event) => {
            event.preventDefault();
            setConnection(draft);
            void models.refetch();
          }}
        >
          <div className="grid gap-4 md:grid-cols-2">
            <FieldBlock label={t("settings:fields.serverUrl")}>
              <Input
                onChange={(event) => setDraft((current) => ({ ...current, serverUrl: event.target.value }))}
                value={draft.serverUrl}
              />
            </FieldBlock>
            <FieldBlock label={t("settings:fields.userId")}>
              <Input
                onChange={(event) => setDraft((current) => ({ ...current, userId: event.target.value }))}
                value={draft.userId}
              />
            </FieldBlock>
          </div>
          <FieldBlock label={t("settings:fields.authToken")}>
            <Input
              autoComplete="current-password"
              onChange={(event) => setDraft((current) => ({ ...current, authToken: event.target.value }))}
              type="password"
              value={draft.authToken}
            />
          </FieldBlock>
          <div className="grid gap-4 md:grid-cols-3">
            <ToggleRow
              description={t("settings:fields.fileReadsDetail")}
              label={t("settings:fields.fileReads")}
              onToggle={() => setSafeFileReads((current) => !current)}
              value={safeFileReads}
            />
            <ToggleRow
              description={t("settings:fields.fileWritesDetail")}
              label={t("settings:fields.fileWrites")}
              onToggle={() => setSafeFileWrites((current) => !current)}
              value={safeFileWrites}
            />
            <ToggleRow
              description={t("settings:fields.terminalCommandsDetail")}
              label={t("settings:fields.terminalCommands")}
              onToggle={() => setTerminalApproval((current) => !current)}
              value={terminalApproval}
            />
          </div>
          <FieldBlock label={t("settings:detectedWorkspace")}>
            <Input disabled value={__TALOS_DEFAULT_PROJECT_ROOT__} />
          </FieldBlock>
          <div className="flex items-center justify-end gap-3">
            <Button
              onClick={() => {
                setDraft(connection);
                setLocale("en");
                setSafeFileReads(true);
                setSafeFileWrites(false);
                setTerminalApproval(true);
              }}
              type="button"
              variant="ghost"
            >
              {t("settings:reset")}
            </Button>
            <Button type="submit">
              <CheckCircle2 size={16} />
              {t("common:save")}
            </Button>
          </div>
        </form>
      </Panel>

      <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/88 p-6">
        <SectionHeader
          icon={<ServerCog size={18} />}
          subtitle={t("settings:workspaceDocsDetail")}
          title={t("settings:workspaceDocsTitle")}
        />

        <div className="mt-6 grid gap-5 md:grid-cols-[240px_minmax(0,1fr)]">
          <div className="space-y-2 rounded-[var(--radius-panel)] border border-border/70 bg-background/38 p-2">
            {DOC_KINDS.map((kind) => (
              <button
                className={[
                  "w-full rounded-[var(--radius-control)] px-3.5 py-3 text-left transition",
                  selectedDocKind === kind
                    ? "bg-background/82 text-ink shadow-[0_10px_24px_rgba(95,84,64,0.045)]"
                    : "text-muted hover:bg-background/58",
                ].join(" ")}
                key={kind}
                onClick={() => setSelectedDocKind(kind)}
                type="button"
              >
                <div className="font-medium">{docLabelMap[kind]}</div>
                <div className="mt-1 text-xs text-muted">{kind}</div>
              </button>
            ))}
          </div>

          <div className="rounded-[var(--radius-panel)] border border-border/70 bg-background/36 p-5">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div>
                <div className="text-sm font-medium text-ink">{docLabelMap[selectedDocKind]}</div>
                <div className="mt-1 text-xs text-muted">
                  {docDetail.exists
                    ? t("settings:workspaceDocUpdated", {
                        date: docDetail.updated_at ? new Date(docDetail.updated_at).toLocaleString() : "-",
                      })
                    : t("settings:workspaceDocMissing")}
                </div>
              </div>
              <Button disabled={docBusy || docDraft === docDetail.content} onClick={() => void saveDoc()}>
                <CheckCircle2 size={16} />
                {docBusy ? t("settings:workspaceDocSaving") : t("common:save")}
              </Button>
            </div>

            <Textarea
              className="mt-4 min-h-[340px] rounded-[var(--radius-panel)] border-border/70 bg-[linear-gradient(180deg,rgba(255,255,255,0.34),rgba(255,255,255,0.14))] font-mono text-sm leading-6"
              onChange={(event) => setDocDraft(event.target.value)}
              value={docDraft}
            />

            {docError ? (
              <div className="mt-3 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                {docError}
              </div>
            ) : null}
          </div>
        </div>
      </Panel>
    </div>
  );
}
