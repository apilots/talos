import { Pencil, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

interface CustomProviderHeaderProps {
  name: string;
  deleteDisabled: boolean;
  onEdit: () => void;
  onDelete: () => void;
}

export function CustomProviderHeader({ name, deleteDisabled, onEdit, onDelete }: CustomProviderHeaderProps) {
  const { t } = useTranslation("settings");
  return (
    <div className="mb-3 flex items-center justify-between gap-3 text-sm text-ink">
      <span className="min-w-0 break-words">{name}</span>
      <div className="flex shrink-0 gap-1">
        <button aria-label={t("modelsPanel.editProvider")} className="rounded-full p-2 text-muted outline-none transition hover:bg-surface focus-visible:ring-2 focus-visible:ring-brand/20" onClick={(event) => { event.currentTarget.focus(); onEdit(); }} type="button">
          <Pencil size={14} />
        </button>
        <button aria-label={t("modelsPanel.deleteProvider")} className="rounded-full p-2 text-muted outline-none transition hover:bg-danger/10 hover:text-danger focus-visible:ring-2 focus-visible:ring-brand/20 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted" disabled={deleteDisabled} onClick={(event) => { event.currentTarget.focus(); onDelete(); }} type="button">
          <Trash2 size={14} />
        </button>
      </div>
    </div>
  );
}
