import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { AutomationMode, AutomationTurnOrigin } from "@talos/sdk-browser";
import { AutomationModeIcon } from "@/shared/ui/AutomationModeIcon";
import { useCopyText } from "@/shared/hooks/useCopyText";
import { MessageAttachments } from "./MessageAttachments";

export function UserInputMessage({ text, fileIds = [], messageId, createdAt, automationOrigin, activationMode }: {
  text: string; fileIds?: string[]; messageId?: string; createdAt?: number; automationOrigin?: AutomationTurnOrigin; activationMode?: AutomationMode;
}) {
  const { t, i18n } = useTranslation("chat");
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const revealed = hovered || focused;
  const { copied, copy } = useCopyText(text);
  const date = createdAt === undefined ? undefined : new Date(createdAt * 1000);
  const knownDate = date && Number.isFinite(date.valueOf()) ? date : undefined;
  const sourceMode = automationOrigin?.type ?? activationMode;
  const sourceLabel = sourceMode ? t(sourceMode === "goal" ? "sentAsGoal" : "sentAsLoop") : undefined;
  return <div className="flex justify-end" data-chat-user-block-id={messageId}>
    <div role="group" aria-label={t("userInputMessage")} tabIndex={0} data-testid="user-input-message" data-input-details-open={revealed}
      className="flex min-w-0 max-w-[min(100%,39rem)] flex-col items-end rounded-[var(--radius-panel)] outline-none focus-visible:ring-2 focus-visible:ring-brand/25"
      onPointerEnter={(event) => { if (event.pointerType !== "touch") setHovered(true); }}
      onPointerLeave={() => setHovered(false)}
      onFocusCapture={() => setFocused(true)}
      onBlurCapture={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocused(false); }}
      onPointerDown={(event) => {
        if (event.pointerType === "touch" && !(event.target instanceof Element && event.target.closest("button,a,input,textarea,select,[role=button]"))) {
          setHovered(false);
          event.currentTarget.focus({ preventScroll: true });
        }
      }}>
      <div data-testid="user-input-body" className="min-w-0 max-w-full rounded-[var(--radius-panel)] border border-border/70 bg-surface px-4 py-3 text-ink">
        <div className="space-y-2.5">
          {fileIds.length ? <MessageAttachments fileIds={fileIds} /> : null}
          {text ? <div className="whitespace-pre-wrap text-sm leading-[1.7] [overflow-wrap:anywhere]">{text}</div> : null}
        </div>
      </div>
      <div data-testid="user-input-metadata" className="mt-1 flex min-h-6 max-w-full flex-wrap items-center justify-end gap-x-2 gap-y-1 text-[10px] text-muted/80">
        {revealed && knownDate ? <time dateTime={knownDate.toISOString()} title={knownDate.toLocaleString(i18n.resolvedLanguage)} className="whitespace-nowrap tabular-nums">{knownDate.toLocaleString(i18n.resolvedLanguage, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</time> : null}
        {revealed && text ? <button type="button" aria-label={t(copied ? "copiedBlock" : "copyBlock")} title={t(copied ? "copiedBlock" : "copyBlock")} onClick={() => void copy()}
          className="flex size-6 items-center justify-center rounded-md text-muted/80 transition-colors hover:bg-hover hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/25">{copied ? <Check size={14} strokeWidth={1.3} /> : <Copy size={14} strokeWidth={1.3} />}</button> : null}
        {sourceMode ? <span data-testid="automation-input-source" aria-label={sourceLabel} title={sourceLabel} className="inline-flex items-center gap-1 whitespace-nowrap"><AutomationModeIcon mode={sourceMode} size={14} /><span>{sourceLabel}</span></span> : null}
      </div>
    </div>
  </div>;
}
