import { useEffect, useState } from "react";
import { ArrowLeft, FolderOpen, TriangleAlert, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { FsDirectoryCreateResponse, FsDirectoryEntry, FsDirectorySnapshot } from "@talos/sdk-browser";

import { cn } from "@/shared/lib/cn";

export type ProjectDirectoryDialogMode = "existing" | "create";

interface ProjectDirectoryDialogProps {
  mode: ProjectDirectoryDialogMode | null;
  onClose: () => void;
  onCreateProject: (input: { directory: string; name?: string }) => Promise<void>;
  readDirectory: (path?: string | null) => Promise<FsDirectorySnapshot>;
  createDirectory: (parent: string, name: string) => Promise<FsDirectoryCreateResponse>;
}

const LAST_DIRECTORY_KEY = "talos.projectDirectoryPicker.lastDirectory";

function deriveNameFromPath(path: string) {
  const normalized = path.trim().replace(/\/+$/, "");
  return normalized.split("/").filter(Boolean).at(-1) || "project";
}

function directoryErrorMessage(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

export function ProjectDirectoryDialog({
  mode,
  onClose,
  onCreateProject,
  readDirectory,
  createDirectory,
}: ProjectDirectoryDialogProps) {
  const { t } = useTranslation(["chat", "common"]);
  const [snapshot, setSnapshot] = useState<FsDirectorySnapshot | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [newDirectoryName, setNewDirectoryName] = useState("");
  const [newProjectName, setNewProjectName] = useState("");
  const [createdDirectoryPath, setCreatedDirectoryPath] = useState<string | null>(null);

  const open = mode !== null;

  useEffect(() => {
    if (!open) {
      return;
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }

    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose, open]);

  useEffect(() => {
    if (!mode) {
      setSnapshot(null);
      setPathInput("");
      setError(null);
      setLoading(false);
      setSubmitting(false);
      setNewDirectoryName("");
      setNewProjectName("");
      setCreatedDirectoryPath(null);
      return;
    }

    setError(null);
    setLoading(true);
    setSubmitting(false);
    setNewDirectoryName("");
    setNewProjectName("");
    setCreatedDirectoryPath(null);

    const lastDirectory = window.localStorage.getItem(LAST_DIRECTORY_KEY);
    void readDirectory(lastDirectory || undefined)
      .then((nextSnapshot) => {
        setSnapshot(nextSnapshot);
        setPathInput(nextSnapshot.path);
      })
      .catch((readError: unknown) => {
        setSnapshot(null);
        setPathInput(lastDirectory ?? "");
        setError(directoryErrorMessage(readError));
      })
      .finally(() => setLoading(false));
  }, [mode, readDirectory]);

  if (!mode) {
    return null;
  }

  async function loadDirectory(path?: string | null) {
    setError(null);
    setLoading(true);
    try {
      const nextSnapshot = await readDirectory(path);
      setSnapshot(nextSnapshot);
      setPathInput(nextSnapshot.path);
    } catch (loadError) {
      setError(directoryErrorMessage(loadError));
    } finally {
      setLoading(false);
    }
  }

  async function enterDirectory(entry: FsDirectoryEntry) {
    if (entry.is_symlink) {
      setError(t("chat:symlinkNotSupported"));
      return;
    }
    await loadDirectory(entry.path);
  }

  async function useExistingDirectory() {
    if (!snapshot) {
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await onCreateProject({
        directory: snapshot.path,
        name: deriveNameFromPath(snapshot.path),
      });
      window.localStorage.setItem(LAST_DIRECTORY_KEY, snapshot.path);
      onClose();
    } catch (submitError) {
      setError(directoryErrorMessage(submitError));
    } finally {
      setSubmitting(false);
    }
  }

  async function createDirectoryProject() {
    if ((!snapshot || !newDirectoryName.trim()) && !createdDirectoryPath) {
      return;
    }
    setSubmitting(true);
    setError(null);
    let directoryForRegistration: string | null = null;
    try {
      directoryForRegistration = createdDirectoryPath ?? (await createDirectory(snapshot!.path, newDirectoryName.trim())).path;
      setCreatedDirectoryPath(directoryForRegistration);
      await onCreateProject({
        directory: directoryForRegistration,
        name: newProjectName.trim() || deriveNameFromPath(directoryForRegistration),
      });
      window.localStorage.setItem(LAST_DIRECTORY_KEY, directoryForRegistration);
      onClose();
    } catch (submitError) {
      const prefix = directoryForRegistration ? t("chat:createdDirectoryRegistrationFailed") : null;
      setError(prefix ? `${prefix} ${directoryErrorMessage(submitError)}` : directoryErrorMessage(submitError));
    } finally {
      setSubmitting(false);
    }
  }

  const title = mode === "existing" ? t("chat:useExistingFolder") : t("chat:startFromScratch");

  return (
    <div className="fixed inset-0 z-[70]">
      <div className="absolute inset-0" onClick={onClose} role="presentation" />
      <div className="pointer-events-none absolute inset-y-0 left-0 right-0 z-10 flex items-center justify-center px-4 py-10 lg:left-[280px]">
        <div className="pointer-events-auto flex max-h-[calc(100vh-80px)] w-[min(680px,calc(100vw-32px))] flex-col overflow-hidden rounded-[calc(var(--radius-panel)+2px)] border border-border/75 bg-card shadow-[0_24px_80px_rgba(0,0,0,0.26)]">
          <div className="flex items-center gap-3 border-b border-border/60 px-4 py-3">
            <div className="flex size-8 items-center justify-center rounded-full border border-border/60 bg-card text-muted">
              <FolderOpen size={15} />
            </div>
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-semibold text-ink">{title}</div>
            </div>
            <button
              className="flex size-8 items-center justify-center rounded-full border border-border/75 bg-card text-muted transition hover:border-brand/30 hover:text-ink"
              onClick={onClose}
              type="button"
            >
              <X size={15} />
            </button>
          </div>

          <div className="flex min-h-0 flex-1 flex-col gap-3 bg-card p-4">
            <form
              className="flex gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                void loadDirectory(pathInput);
              }}
            >
              <input
                className="min-w-0 flex-1 rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none placeholder:text-muted focus:border-brand/50 focus:ring-0"
                onChange={(event) => setPathInput(event.target.value)}
                value={pathInput}
              />
              <button
                className="rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none transition hover:border-brand/50 focus:outline-none"
                disabled={loading}
                type="submit"
              >
                {loading ? t("common:loading") : t("chat:openPath")}
              </button>
              <button
                className="rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none transition hover:border-brand/50 focus:outline-none"
                onClick={() => void loadDirectory(null)}
                type="button"
              >
                {t("chat:openHome")}
              </button>
            </form>

            {error ? (
              <div className="rounded-xl border border-danger/20 bg-danger/5 px-3 py-2 text-[12px] leading-snug text-danger">
                {error}
              </div>
            ) : null}

          <div className="min-h-[220px] flex-1 overflow-y-auto rounded-xl border border-border/45 bg-card p-1">
            {snapshot?.parent ? (
              <button
                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] text-muted transition hover:bg-muted/10 hover:text-ink"
                onClick={() => void loadDirectory(snapshot.parent)}
                type="button"
              >
                <ArrowLeft size={13} />
                {t("chat:parentDirectory")}
              </button>
            ) : null}
            {loading ? (
              <div className="px-2 py-10 text-center text-[13px] text-muted">{t("common:loading")}</div>
            ) : snapshot && snapshot.entries.length > 0 ? (
              snapshot.entries.map((entry) => (
                <button
                  className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] transition hover:bg-muted/10"
                  key={entry.path}
                  onClick={() => void enterDirectory(entry)}
                  title={entry.is_symlink ? t("chat:symlinkNotSupported") : entry.path}
                  type="button"
                >
                  <FolderOpen className={cn("shrink-0", entry.is_symlink ? "text-danger" : "text-muted")} size={13} />
                  <span className="min-w-0 flex-1 truncate">{entry.name}</span>
                  {entry.is_symlink ? <TriangleAlert className="shrink-0 text-danger" size={13} /> : null}
                </button>
              ))
            ) : (
              <div className="px-2 py-10 text-center text-[13px] text-muted">{t("chat:noDirectories")}</div>
            )}
          </div>

          {mode === "create" ? (
            <div className="grid gap-2 border-t border-border/50 pt-3">
              <input
                className="rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none placeholder:text-muted focus:border-brand/50 focus:ring-0"
                disabled={Boolean(createdDirectoryPath)}
                onChange={(event) => {
                  setCreatedDirectoryPath(null);
                  setNewDirectoryName(event.target.value);
                }}
                placeholder={t("chat:newFolderName")}
                value={newDirectoryName}
              />
              <input
                className="rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none placeholder:text-muted focus:border-brand/50 focus:ring-0"
                onChange={(event) => setNewProjectName(event.target.value)}
                placeholder={t("chat:projectNameOptional")}
                value={newProjectName}
              />
              <button
                className="rounded-xl border border-brand/25 bg-brand/10 px-3 py-2 text-[13px] font-medium text-brand outline-none transition hover:bg-brand/15 disabled:cursor-not-allowed disabled:opacity-50"
                disabled={(!newDirectoryName.trim() && !createdDirectoryPath) || submitting}
                onClick={() => void createDirectoryProject()}
                type="button"
              >
                {submitting
                  ? t("common:loading")
                  : createdDirectoryPath
                    ? t("chat:retryProjectRegistration")
                    : t("chat:createAndUseProject")}
              </button>
            </div>
          ) : (
            <div className="flex justify-end border-t border-border/50 pt-3">
              <button
                className="rounded-xl border border-brand/25 bg-brand/10 px-4 py-2 text-[13px] font-medium text-brand outline-none transition hover:bg-brand/15 disabled:cursor-not-allowed disabled:opacity-50"
                disabled={!snapshot || submitting}
                onClick={() => void useExistingDirectory()}
                type="button"
              >
                {submitting ? t("common:loading") : t("chat:useThisFolder")}
              </button>
            </div>
          )}
        </div>
        </div>
      </div>
    </div>
  );
}
