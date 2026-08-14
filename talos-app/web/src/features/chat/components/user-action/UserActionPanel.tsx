import type {
  TalosPendingUserAction,
  UserActionResponse,
} from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import { QuestionPanel } from "./QuestionPanel";
import { SandboxApprovalPanel } from "./SandboxApprovalPanel";

interface UserActionPanelProps {
  action: TalosPendingUserAction;
  disabled?: boolean;
  error?: string;
  onSubmit: (response: UserActionResponse) => void;
}

export function UserActionPanel({
  action,
  disabled = false,
  error,
  onSubmit,
}: UserActionPanelProps) {
  const { t } = useTranslation("chat");
  const isActionable = action.state === "pending";
  let content;
  switch (action.params.type) {
    case "question":
      content = (
        <QuestionPanel
          disabled={disabled || !isActionable}
          error={error}
          onSubmit={(answer) => onSubmit({ type: "question", response: answer })}
          params={action.params.params}
        />
      );
      break;
    case "sandboxApproval":
      content = (
        <SandboxApprovalPanel
          disabled={disabled || !isActionable}
          error={error}
          onSubmit={(decision) =>
            onSubmit({ type: "sandboxApproval", response: decision })
          }
          params={action.params.params}
        />
      );
      break;
    case "dynamicToolResult":
      return null;
  }
  return (
    <>
      {content}
      {!isActionable ? (
        <p className="mt-2 pl-4 text-[12px] text-muted" data-user-action-state={action.state}>
          {t(`userActionState.${action.state}`)}
        </p>
      ) : null}
    </>
  );
}
