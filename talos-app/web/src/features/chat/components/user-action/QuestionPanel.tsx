import { useState } from "react";
import type { QuestionAnswer } from "@talos/sdk-browser";
import type { AppProtocol } from "@talos/sdk-core";
import { Send } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";
import { Textarea } from "@/shared/ui/Input";

interface QuestionPanelProps {
  params: AppProtocol.QuestionParams;
  disabled: boolean;
  error?: string;
  onSubmit: (answer: QuestionAnswer) => void;
}

export function QuestionPanel({ params, disabled, error, onSubmit }: QuestionPanelProps) {
  const { t } = useTranslation("chat");
  const [showOther, setShowOther] = useState(false);
  const [text, setText] = useState("");

  return (
    <section className="mt-3 border-l-2 border-brand/35 pl-4" data-user-action="question">
      <header className="mb-3">
        <div className="min-w-0">
          <p className="text-[12px] font-semibold text-muted">{params.header}</p>
          <p className="mt-1 text-sm leading-6 text-ink">{params.question}</p>
        </div>
      </header>
      <div className="grid grid-cols-1 gap-2">
        {params.options.map((option) => (
          <button
            className="min-w-0 rounded-[var(--radius-control)] border border-border/75 px-3 py-2.5 text-left transition hover:border-brand/45 hover:bg-brand/5 disabled:cursor-not-allowed disabled:opacity-50"
            disabled={disabled}
            key={option.id}
            onClick={() =>
              onSubmit({ type: "selectedOption", optionId: option.id })
            }
            type="button"
          >
            <span className="block text-sm font-medium text-ink">{option.label}</span>
            <span className="mt-0.5 block text-[12px] leading-5 text-muted">
              {option.description}
            </span>
          </button>
        ))}
        <button
          className="rounded-[var(--radius-control)] border border-dashed border-border/75 px-3 py-2.5 text-left text-sm text-muted transition hover:border-brand/45 hover:text-ink disabled:cursor-not-allowed disabled:opacity-50"
          disabled={disabled}
          onClick={() => setShowOther(true)}
          type="button"
        >
          {t("userActionOther")}
        </button>
      </div>
      {showOther ? (
        <div className="mt-3 flex items-end gap-2">
          <Textarea
            aria-label={t("userActionOther")}
            className="min-h-20 resize-y"
            disabled={disabled}
            onChange={(event) => setText(event.target.value)}
            value={text}
          />
          <Button
            aria-label={t("userActionSubmit")}
            disabled={disabled || !text.trim()}
            onClick={() => onSubmit({ type: "freeForm", text: text.trim() })}
            type="button"
          >
            <Send size={14} />
          </Button>
        </div>
      ) : null}
      {error ? <p className="mt-2 text-[12px] text-danger">{error}</p> : null}
    </section>
  );
}
