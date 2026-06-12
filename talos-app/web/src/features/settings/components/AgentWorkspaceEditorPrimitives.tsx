import { useTranslation } from "react-i18next";

import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";

export type EditorMode = "preview" | "edit";

export function EditorModeSwitch({
  mode,
  setMode,
}: {
  mode: EditorMode;
  setMode: (mode: EditorMode) => void;
}) {
  const { t } = useTranslation("common");

  return (
    <div className="flex items-center gap-2 rounded-[var(--radius-control)] border border-border/70 bg-background/48 p-1">
      <button
        className={[
          "rounded-[var(--radius-control)] px-3 py-2 text-sm transition",
          mode === "preview" ? "bg-card text-ink shadow-panel" : "text-muted hover:text-ink",
        ].join(" ")}
        onClick={() => setMode("preview")}
        type="button"
      >
        {t("preview")}
      </button>
      <button
        className={[
          "rounded-[var(--radius-control)] px-3 py-2 text-sm transition",
          mode === "edit" ? "bg-card text-ink shadow-panel" : "text-muted hover:text-ink",
        ].join(" ")}
        onClick={() => setMode("edit")}
        type="button"
      >
        {t("edit")}
      </button>
    </div>
  );
}

export function MarkdownPreview({ text, emptyText }: { text: string; emptyText: string }) {
  return (
    <div className="mt-4 rounded-[var(--radius-panel)] border border-border/70 bg-background/36 p-5">
      {text.trim() ? (
        <MarkdownRenderer className="markdown-body prose prose-sm max-w-none" text={text} />
      ) : (
        <div className="text-sm text-muted">{emptyText}</div>
      )}
    </div>
  );
}
