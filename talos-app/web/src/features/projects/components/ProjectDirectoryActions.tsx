import type { FsDirectorySnapshot } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import type { ProjectDirectoryDialogMode } from "./ProjectDirectoryDialog";

interface ProjectDirectoryActionsProps {
  mode: ProjectDirectoryDialogMode;
  snapshot: FsDirectorySnapshot | null;
  submitting: boolean;
  newDirectoryName: string;
  newProjectName: string;
  createdDirectoryPath: string | null;
  onNewDirectoryNameChange: (value: string) => void;
  onNewProjectNameChange: (value: string) => void;
  onResetCreatedDirectoryPath: () => void;
  onCreateDirectoryProject: () => void;
  onUseExistingDirectory: () => void;
}

export function ProjectDirectoryActions({
  mode,
  snapshot,
  submitting,
  newDirectoryName,
  newProjectName,
  createdDirectoryPath,
  onNewDirectoryNameChange,
  onNewProjectNameChange,
  onResetCreatedDirectoryPath,
  onCreateDirectoryProject,
  onUseExistingDirectory,
}: ProjectDirectoryActionsProps) {
  const { t } = useTranslation(["chat", "common"]);

  if (mode === "create") {
    return (
      <div className="grid gap-2 border-t border-border/50 pt-3">
        <input
          className="rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none placeholder:text-muted focus:border-brand/50 focus:ring-0"
          disabled={Boolean(createdDirectoryPath)}
          onChange={(event) => {
            onResetCreatedDirectoryPath();
            onNewDirectoryNameChange(event.target.value);
          }}
          placeholder={t("chat:newFolderName")}
          value={newDirectoryName}
        />
        <input
          className="rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none placeholder:text-muted focus:border-brand/50 focus:ring-0"
          onChange={(event) => onNewProjectNameChange(event.target.value)}
          placeholder={t("chat:projectNameOptional")}
          value={newProjectName}
        />
        <button
          className="rounded-xl border border-brand/25 bg-brand/10 px-3 py-2 text-[13px] font-medium text-brand outline-none transition hover:bg-brand/15 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={(!newDirectoryName.trim() && !createdDirectoryPath) || submitting}
          onClick={onCreateDirectoryProject}
          type="button"
        >
          {submitting
            ? t("common:loading")
            : createdDirectoryPath
              ? t("chat:retryProjectRegistration")
              : t("chat:createAndUseProject")}
        </button>
      </div>
    );
  }

  return (
    <div className="flex justify-end border-t border-border/50 pt-3">
      <button
        className="rounded-xl border border-brand/25 bg-brand/10 px-4 py-2 text-[13px] font-medium text-brand outline-none transition hover:bg-brand/15 disabled:cursor-not-allowed disabled:opacity-50"
        disabled={!snapshot || submitting}
        onClick={onUseExistingDirectory}
        type="button"
      >
        {submitting ? t("common:loading") : t("chat:useThisFolder")}
      </button>
    </div>
  );
}
