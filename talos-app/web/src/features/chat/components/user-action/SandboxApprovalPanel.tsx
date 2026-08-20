import type {
  SandboxApprovalDecision,
} from "@talos/sdk-browser";
import type { AppProtocol } from "@talos/sdk-core";
import { ShieldAlert } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";

interface SandboxApprovalPanelProps {
  params: AppProtocol.SandboxApprovalParams;
  disabled: boolean;
  error?: string;
  onSubmit: (decision: SandboxApprovalDecision) => void;
}

export function SandboxApprovalPanel({
  params,
  disabled,
  error,
  onSubmit,
}: SandboxApprovalPanelProps) {
  const { t } = useTranslation("chat");
  return (
    <section className="mt-3 border-l-2 border-warning/55 pl-4" data-user-action="sandbox-approval">
      <header className="flex items-start gap-2.5">
        <ShieldAlert className="mt-0.5 shrink-0 text-warning" size={16} />
        <div className="min-w-0">
          <p className="text-sm font-semibold text-ink">{t("sandboxApprovalTitle")}</p>
          <p className="mt-1 text-[12px] leading-5 text-muted">
            {approvalReason(params.reason, t)}
          </p>
        </div>
      </header>
      <ApprovalScope params={params} />
      <div className="mt-3 flex flex-wrap gap-2">
        {params.availableDecisions.map((decision) => (
          <Button
            disabled={disabled}
            key={decision}
            onClick={() => onSubmit(decision)}
            type="button"
            variant={decision === "allowOnce" ? "primary" : decision === "deny" ? "secondary" : "ghost"}
          >
            {t(`sandboxDecision.${decision}`)}
          </Button>
        ))}
      </div>
      {error ? <p className="mt-2 text-[12px] text-danger">{error}</p> : null}
    </section>
  );
}

function ApprovalScope({ params }: { params: AppProtocol.SandboxApprovalParams }) {
  const presentation = params.presentation;
  switch (presentation.type) {
    case "command":
      return (
        <div className="mt-3 overflow-x-auto rounded-[var(--radius-control)] bg-surface/90 px-3 py-2 font-mono text-[12px] leading-5 text-ink">
          <div className="text-muted">{presentation.cwd}</div>
          <div>{presentation.command}</div>
          {presentation.riskSummary ? (
            <div className="mt-1 font-sans text-warning">{presentation.riskSummary}</div>
          ) : null}
        </div>
      );
    case "fileSystem":
      return (
        <div className="mt-3 space-y-1 font-mono text-[12px] text-ink">
          {presentation.paths.map((path) => <div className="break-all" key={path}>{path}</div>)}
        </div>
      );
    case "network":
      return <div className="mt-3 break-all font-mono text-[12px] text-ink">{presentation.scope}</div>;
  }
}

function approvalReason(
  reason: AppProtocol.SandboxApprovalReason,
  t: (key: string) => string,
) {
  return t(`sandboxReason.${reason}`);
}
