import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { FsDirectoryCreateResponse, FsDirectoryEntry, FsDirectorySnapshot } from "@talos/sdk-browser";

import { ProjectDirectoryActions } from "./ProjectDirectoryActions";
import { ProjectDirectoryBrowser } from "./ProjectDirectoryBrowser";

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
      <div className="absolute inset-0 bg-black/30 backdrop-blur-[2px]" onClick={onClose} role="presentation" />
      <div className="pointer-events-none absolute inset-y-0 left-0 right-0 z-10 flex items-center justify-center px-4 py-10 lg:left-[280px]">
        <div className="pointer-events-auto flex max-h-[calc(100vh-80px)] w-[min(680px,calc(100vw-32px))] flex-col overflow-hidden rounded-[var(--radius-panel)] border border-border bg-card shadow-panel">
          <div className="flex items-center gap-3 border-b border-border/60 px-4 py-3">
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-semibold text-ink">{title}</div>
            </div>
            <button
              aria-label={t("common:close")}
              className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-surface hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/30"
              onClick={onClose}
              type="button"
            >
              <X size={15} />
            </button>
          </div>

          <div className="flex min-h-0 flex-1 flex-col gap-3 bg-card p-4">
            <ProjectDirectoryBrowser
              error={error}
              loading={loading}
              onEnterDirectory={(entry) => void enterDirectory(entry)}
              onLoadDirectory={(path) => void loadDirectory(path)}
              onPathInputChange={setPathInput}
              pathInput={pathInput}
              snapshot={snapshot}
            />
            <ProjectDirectoryActions
              createdDirectoryPath={createdDirectoryPath}
              mode={mode}
              newDirectoryName={newDirectoryName}
              newProjectName={newProjectName}
              onCreateDirectoryProject={() => void createDirectoryProject()}
              onNewDirectoryNameChange={setNewDirectoryName}
              onNewProjectNameChange={setNewProjectName}
              onResetCreatedDirectoryPath={() => setCreatedDirectoryPath(null)}
              onUseExistingDirectory={() => void useExistingDirectory()}
              snapshot={snapshot}
              submitting={submitting}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
