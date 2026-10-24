import { documentOverlayClasses } from "@/shared/ui/overlayStyles";
import { useEnvironmentKey } from "@/features/computers";
import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { FileText, Loader2, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { PluginDocument, SkillSummary } from "@talos/sdk-browser";

import { readLocalSkill } from "@/features/plugins/api";
import { skillTitle } from "@/features/plugins/lib/pluginDisplay";
import { PluginDocumentBody } from "./PluginDocumentBody";

interface SkillDocumentDialogProps {
  skill: SkillSummary | null;
  onClose: () => void;
}

export function SkillDocumentDialog({ skill, onClose }: SkillDocumentDialogProps) {
  const environmentKey = useEnvironmentKey();
  const { t } = useTranslation("plugins");
  const documentQuery = useQuery({
    enabled: Boolean(skill),
    queryKey: ["talos", environmentKey, "skills", "document", skill?.path],
    queryFn: () => readLocalSkill(skill as SkillSummary),
  });

  useEffect(() => {
    if (!skill) {
      return;
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, skill]);

  if (!skill) {
    return null;
  }

  const readSkill = documentQuery.data?.document.summary ?? skill;
  const document = documentQuery.data
    ? skillDocumentToPluginDocument(documentQuery.data.document.contents, readSkill.path)
    : null;

  return (
    <div className={documentOverlayClasses.frame}>
      <button
        aria-label={t("dialog.closeSkillBackdrop")}
        className={documentOverlayClasses.backdrop}
        onClick={onClose}
        type="button"
      />
      <section className="relative flex max-h-[86vh] w-full max-w-[860px] flex-col overflow-hidden rounded-[var(--radius-panel)] border border-border bg-card shadow-panel">
        <header className="flex items-start justify-between gap-4 border-b border-border px-6 py-5">
          <div className="flex min-w-0 items-start gap-3">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-[0.7rem] bg-card text-brand">
              <FileText size={18} />
            </div>
            <div className="min-w-0">
              <h2 className="truncate text-lg font-semibold text-ink">{skillTitle(readSkill)}</h2>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted">
                <span className="rounded-full bg-card px-2 py-0.5">{t("localSkills.heading")}</span>
                {readSkill.read_only ? (
                  <span className="rounded-full bg-card px-2 py-0.5">{t("localSkills.readOnly")}</span>
                ) : null}
                <span className="min-w-0 truncate">{compactHomePath(readSkill.path)}</span>
              </div>
            </div>
          </div>
          <button
            aria-label={t("dialog.closeSkill")}
            className="flex size-9 shrink-0 items-center justify-center rounded-[0.7rem] text-muted transition hover:bg-hover hover:text-ink"
            onClick={onClose}
            type="button"
          >
            <X size={18} />
          </button>
        </header>
        <div className="min-h-[340px] overflow-y-auto px-6 py-5">
          {documentQuery.isLoading ? (
            <div className="flex h-40 items-center justify-center gap-2 text-sm text-muted">
              <Loader2 className="animate-spin" size={16} />
              {t("dialog.loadingSkill")}
            </div>
          ) : documentQuery.isError ? (
            <div className="rounded-[0.8rem] border border-border bg-card px-4 py-3 text-sm text-muted">
              {t("dialog.failedLoadSkill")}
            </div>
          ) : document ? (
            <PluginDocumentBody document={document} />
          ) : (
            <div className="rounded-[0.8rem] border border-border bg-card px-4 py-3 text-sm text-muted">
              {t("dialog.noSkillDocument")}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}

function skillDocumentToPluginDocument(contents: string, path: string): PluginDocument {
  return {
    role: "primary",
    path,
    title: "SKILL.md",
    media_type: "markdown",
    contents,
  };
}

function compactHomePath(path: string) {
  if (typeof window === "undefined") {
    return path;
  }
  return path.replace(/^\/Users\/[^/]+/, "~");
}
