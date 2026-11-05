import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import { describeProjectDirectory, isUnboundProject } from "@/shared/types/domain";
import type { ChatComposerProps, ReasoningLevel } from "./ChatComposer.types";
import { ComposerModelPicker } from "./ComposerModelPicker";

type Props = Pick<ChatComposerProps, "disabled" | "models" | "onModelChange" | "onReasoningLevelChange" | "selectedModel" | "onProjectChange" | "activeProjectId" | "projects"> & {
  isBusy: boolean;
  modelTriggerRef: RefObject<HTMLButtonElement | null>;
  setModelPickerOpen: (open: boolean) => void;
  reasoningLevel: ReasoningLevel;
};

export function ComposerCompactContext({ disabled, isBusy, modelTriggerRef, models, onModelChange, setModelPickerOpen, onReasoningLevelChange, reasoningLevel, selectedModel, onProjectChange, activeProjectId, projects }: Props) {
  const { t } = useTranslation("common");
  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-border/70 px-3 py-2">
      <ComposerModelPicker
        disabled={Boolean(disabled || isBusy)}
        modelTriggerRef={modelTriggerRef}
        models={models}
        onModelChange={onModelChange}
        onOpenChange={setModelPickerOpen}
        onReasoningLevelChange={onReasoningLevelChange}
        reasoningLevel={reasoningLevel}
        selectedModel={selectedModel}
      />
      <select
        className="form-control-focus min-w-[12rem] flex-[1.2] rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm transition md:max-w-[22rem]"
        onChange={(event) => onProjectChange(event.target.value)}
        value={activeProjectId}
      >
        {projects.map((project) => (
          <option key={project.id} value={project.id}>
            {isUnboundProject(project.id)
              ? t("common:noProject")
              : `${project.name} · ${describeProjectDirectory(project.directory)}`}
          </option>
        ))}
      </select>
    </div>
  );
}
