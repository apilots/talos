import type { ReactNode } from "react";
import { Check, Copy, ThumbsDown, ThumbsUp } from "lucide-react";
import { useEffect, useRef, useState } from "react";

export function CopyableBlock({
  children,
  copiedLabel,
  copyLabel,
  copyText,
  helpfulLabel,
  notHelpfulLabel,
  responseActionInset = false,
  variant = "hover",
}: {
  children: ReactNode;
  copiedLabel: string;
  copyLabel: string;
  copyText: string;
  helpfulLabel?: string;
  notHelpfulLabel?: string;
  responseActionInset?: boolean;
  variant?: "hover" | "response";
}) {
  const [copied, setCopied] = useState(false);
  const resetTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (resetTimerRef.current) {
        clearTimeout(resetTimerRef.current);
      }
    };
  }, []);

  const copyBlock = async () => {
    if (!copyText) {
      return;
    }
    await writeClipboardText(copyText);
    setCopied(true);
    if (resetTimerRef.current) {
      clearTimeout(resetTimerRef.current);
    }
    resetTimerRef.current = setTimeout(() => setCopied(false), 1200);
  };

  return (
    <div className="group relative">
      {children}
      {variant === "response" ? (
        <div
          className={responseActionInset ? "ml-11 mt-2 flex items-center gap-0.5 md:ml-12" : "mt-2 flex items-center gap-0.5"}
          data-response-actions="true"
        >
          <button
            aria-label={copied ? copiedLabel : copyLabel}
            className="flex size-7 items-center justify-center rounded-md text-muted/75 transition-colors hover:bg-surface hover:text-ink focus:outline-none focus:ring-2 focus:ring-brand/25"
            onClick={() => void copyBlock()}
            title={copied ? copiedLabel : copyLabel}
            type="button"
          >
            {copied ? <Check size={14} /> : <Copy size={14} />}
          </button>
          <button
            aria-label={helpfulLabel}
            className="flex size-7 items-center justify-center rounded-md text-muted/75 transition-colors hover:bg-surface hover:text-ink focus:outline-none focus:ring-2 focus:ring-brand/25"
            data-placeholder-action="true"
            title={helpfulLabel}
            type="button"
          >
            <ThumbsUp size={14} />
          </button>
          <button
            aria-label={notHelpfulLabel}
            className="flex size-7 items-center justify-center rounded-md text-muted/75 transition-colors hover:bg-surface hover:text-ink focus:outline-none focus:ring-2 focus:ring-brand/25"
            data-placeholder-action="true"
            title={notHelpfulLabel}
            type="button"
          >
            <ThumbsDown size={14} />
          </button>
        </div>
      ) : (
        <button
          aria-label={copied ? copiedLabel : copyLabel}
          className="absolute right-2 top-2 z-10 flex size-8 items-center justify-center rounded-[0.7rem] border border-border/70 bg-card text-muted opacity-0 transition hover:border-brand/35 hover:text-brand focus:opacity-100 focus:outline-none focus:ring-2 focus:ring-brand/30 group-hover:opacity-100"
          onClick={() => void copyBlock()}
          title={copied ? copiedLabel : copyLabel}
          type="button"
        >
          {copied ? <Check size={15} /> : <Copy size={15} />}
        </button>
      )}
    </div>
  );
}

async function writeClipboardText(text: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand("copy");
  document.body.removeChild(textarea);
}
