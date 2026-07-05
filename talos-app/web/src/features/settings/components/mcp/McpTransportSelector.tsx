import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";

export type McpTransportMode = "stdio" | "http";

interface McpTransportSelectorProps {
  disabled: boolean;
  mode: McpTransportMode;
  onChange: (mode: McpTransportMode) => void;
}

export function McpTransportSelector({
  disabled,
  mode,
  onChange,
}: McpTransportSelectorProps) {
  const { t } = useTranslation("settings");

  return (
    <div className="grid grid-cols-2 border-y border-border/55 bg-background/40 p-1">
      <TransportButton
        active={mode === "stdio"}
        disabled={disabled}
        label={t("mcpServers.stdio")}
        onClick={() => onChange("stdio")}
      />
      <TransportButton
        active={mode === "http"}
        disabled={disabled}
        label={t("mcpServers.streamableHttp")}
        onClick={() => onChange("http")}
      />
    </div>
  );
}

function TransportButton({
  active,
  disabled,
  label,
  onClick,
}: {
  active: boolean;
  disabled: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      className={cn(
        "h-10 rounded-[0.75rem] text-sm font-semibold transition focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30",
        active ? "bg-muted/45 text-ink" : "text-muted hover:bg-card/60 hover:text-ink",
        disabled && !active ? "cursor-not-allowed opacity-60 hover:bg-transparent hover:text-muted" : null,
      )}
      disabled={disabled && !active}
      onClick={onClick}
      type="button"
    >
      {label}
    </button>
  );
}
