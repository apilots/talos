import { useTranslation } from "react-i18next";
import type { SessionInfo } from "@talos/sdk-browser";
import type { Ref } from "react";

import type { DropdownItem } from "./ComposerDropdown";
import type { ComposerModelOption } from "./ComposerModelSelect";
import type { ReasoningLevel } from "./ChatComposer.types";
import { ComposerContextUsageIndicator } from "./ComposerContextUsageIndicator";
import { ComposerProviderModelDropdowns } from "./ComposerProviderModelDropdowns";
import { ComposerSubmitButton } from "./ComposerSubmitButton";

interface ComposerRunControlsProps {
  canSubmit: boolean;
  compact: boolean;
  contextUsage?: SessionInfo["usage"]["context"];
  disabled?: boolean;
  isBusy: boolean;
  isCancelling: boolean;
  modelTriggerRef?: Ref<HTMLButtonElement>;
  models: ComposerModelOption[];
  onCancel?: () => void;
  onModelChange?: (model: string) => void;
  onModelPickerOpenChange?: (open: boolean) => void;
  onReasoningLevelChange?: (level: ReasoningLevel) => void;
  onSend: () => Promise<void>;
  reasoningItems: DropdownItem[];
  reasoningLevel: ReasoningLevel;
  selectedModel?: string;
  sessionTotalTokens?: number;
}

export function ComposerRunControls({
  canSubmit,
  compact,
  contextUsage,
  disabled,
  isBusy,
  isCancelling,
  modelTriggerRef,
  models,
  onCancel,
  onModelChange,
  onModelPickerOpenChange,
  onReasoningLevelChange,
  onSend,
  reasoningItems,
  reasoningLevel,
  selectedModel,
  sessionTotalTokens,
}: ComposerRunControlsProps) {
  const { t } = useTranslation(["chat"]);

  return (
    <>
      {!compact ? (
        <ComposerProviderModelDropdowns
          disabled={Boolean(disabled || isBusy)}
          effortLabel={t("chat:reasoningLabel")}
          modelLabel={t("chat:modelLabel")}
          models={models}
          onChange={onModelChange}
          onEffortChange={(value) => onReasoningLevelChange?.(value as ReasoningLevel)}
          onOpenChange={onModelPickerOpenChange}
          modelTriggerRef={modelTriggerRef}
          providerLabel={t("chat:providerLabel")}
          reasoningItems={reasoningItems}
          selectedEffort={reasoningLevel}
          selectedModel={selectedModel}
        />
      ) : null}
      {!compact ? (
        <ComposerContextUsageIndicator
          sessionTotalTokens={sessionTotalTokens}
          usage={contextUsage}
        />
      ) : null}
      <ComposerSubmitButton
        canSubmit={canSubmit}
        disabled={disabled}
        isBusy={isBusy}
        isCancelling={isCancelling}
        onCancel={onCancel}
        onSend={onSend}
      />
    </>
  );
}
