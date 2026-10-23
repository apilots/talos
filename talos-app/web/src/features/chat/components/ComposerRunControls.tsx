import type { SessionInfo } from "@talos/sdk-browser";
import type { Ref } from "react";

import type { ComposerModelOption } from "../model-selection";
import type { ReasoningLevel } from "./ChatComposer.types";
import { ComposerContextUsageIndicator } from "./ComposerContextUsageIndicator";
import { ComposerModelPicker } from "./ComposerModelPicker";
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
  reasoningLevel,
  selectedModel,
  sessionTotalTokens,
}: ComposerRunControlsProps) {
  return (
    <div className="ml-auto flex min-w-0 max-w-full flex-[1_1_14rem] items-center justify-end gap-1">
      {!compact ? (
        <ComposerModelPicker
          disabled={Boolean(disabled || isBusy)}
          models={models}
          onModelChange={onModelChange}
          onReasoningLevelChange={onReasoningLevelChange}
          onOpenChange={onModelPickerOpenChange}
          modelTriggerRef={modelTriggerRef}
          reasoningLevel={reasoningLevel}
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
    </div>
  );
}
