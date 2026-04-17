import { LoaderCircle, Paperclip, Send, Square } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";
import { cn } from "@/shared/lib/cn";
import { Panel } from "@/shared/ui/Panel";
import { Textarea } from "@/shared/ui/Input";
import { describeProjectRoot, getProjectRecordKey } from "@/shared/types/domain";
import type { ApprovalMode, ProjectRecord, RuntimeMode } from "@/shared/types/ui";

interface ChatComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: () => Promise<void> | void;
  onCancel?: () => void;
  disabled?: boolean;
  isStreaming?: boolean;
  isCancelling?: boolean;
  models: Array<{ id: string; name: string }>;
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  projects: ProjectRecord[];
  activeProjectId: string;
  onProjectChange: (projectId: string) => void;
  runtimeMode: RuntimeMode;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
  approvalMode: ApprovalMode;
  onApprovalModeChange: (mode: ApprovalMode) => void;
  showProjectSelector?: boolean;
  compact?: boolean;
}

export function ChatComposer({
  draft,
  onDraftChange,
  onSend,
  onCancel,
  disabled,
  isStreaming,
  isCancelling = false,
  models,
  selectedModel,
  onModelChange,
  projects,
  activeProjectId,
  onProjectChange,
  runtimeMode,
  onRuntimeModeChange,
  approvalMode,
  onApprovalModeChange,
  showProjectSelector = true,
  compact = false,
}: ChatComposerProps) {
  const { t } = useTranslation(["chat", "common"]);
  const isBusy = Boolean(isStreaming || isCancelling);

  return (
    <Panel className="sticky bottom-0 overflow-hidden rounded-[1.5rem] border-border/80 bg-card/95 p-2.5 backdrop-blur">
      <div className={compact ? "mb-2 flex flex-wrap items-center gap-2 border-b border-border/70 pb-2" : "mb-2 flex flex-wrap items-center gap-2 border-b border-border/70 pb-2"}>
        <select
          className="min-w-[10rem] flex-1 rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm outline-none transition focus:border-brand/50 md:max-w-[15rem]"
          onChange={(event) => onModelChange?.(event.target.value)}
          value={selectedModel}
        >
          {models.map((model) => (
            <option key={model.id} value={model.id}>
              {model.name}
            </option>
          ))}
        </select>
        {showProjectSelector ? (
          <select
            className="min-w-[12rem] flex-[1.2] rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm outline-none transition focus:border-brand/50 md:max-w-[22rem]"
            onChange={(event) => onProjectChange(event.target.value)}
            value={activeProjectId}
          >
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {getProjectRecordKey(project.root) === "workspace-default"
                  ? t("common:defaultWorkspace")
                  : `${project.name} · ${describeProjectRoot(project.root)}`}
              </option>
            ))}
          </select>
        ) : null}
      </div>
      <Textarea
        className={cn(
          "resize-none border-0 bg-transparent px-0 py-0 text-[14px] leading-[1.6] focus:border-0",
          compact ? "min-h-12" : "min-h-16",
        )}
        disabled={disabled}
        onChange={(event) => onDraftChange(event.target.value)}
        onKeyDown={(event) => {
          if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
            event.preventDefault();
            void onSend();
          }
        }}
        placeholder={t("chat:askTalos")}
        value={draft}
      />
      <div className={cn("mt-2 flex flex-col border-t border-border/70 pt-2", compact ? "gap-1.5" : "gap-2")}>
        <div className="flex flex-wrap items-center gap-2 text-[10px] text-muted">
          <span className="inline-flex items-center gap-2 rounded-full border border-border/80 bg-background/50 px-2.5 py-1.5">
            <Paperclip size={14} />
            {t("chat:attachments")}
          </span>
          <span className="rounded-full border border-border/80 bg-background/50 px-2.5 py-1.5">{t("chat:mentionFiles")}</span>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <select
              aria-label="Runtime mode"
              className="rounded-full border border-border bg-background/60 px-2.5 py-1.5 text-[10px] font-medium text-ink outline-none transition focus:border-brand/50"
              onChange={(event) => onRuntimeModeChange(event.target.value as RuntimeMode)}
              value={runtimeMode}
            >
              <option value="cloud">{t("common:cloud")}</option>
              <option disabled value="local">
                {t("common:localDesktopOnly")}
              </option>
            </select>
            <select
              aria-label="Approval mode"
              className="rounded-full border border-border bg-background/60 px-2.5 py-1.5 text-[10px] font-medium text-ink outline-none transition focus:border-brand/50"
              onChange={(event) => onApprovalModeChange(event.target.value as ApprovalMode)}
              value={approvalMode}
            >
              <option value="needs_approval">{t("chat:defaultPermissions")}</option>
              <option value="skip">{t("chat:fullAccess")}</option>
            </select>
          </div>
          <div className="ml-auto flex items-center gap-3">
            <Button
              disabled={disabled || (!isBusy && draft.trim().length === 0)}
              onClick={() => {
                if (isBusy) {
                  onCancel?.();
                  return;
                }
                void onSend();
              }}
              type="button"
              variant={isBusy ? "danger" : "primary"}
            >
              {isCancelling ? (
                <LoaderCircle className="animate-spin" size={15} />
              ) : isBusy ? (
                <Square size={15} />
              ) : (
                <Send size={15} />
              )}
              {isCancelling ? t("chat:stopping") : isBusy ? t("chat:stop") : t("chat:send")}
            </Button>
          </div>
        </div>
      </div>
    </Panel>
  );
}
