import type { AppProtocol } from "@talos/sdk-core";
import { apiErrorMessage } from "./apiErrorMessage";
import { i18n } from "./setup";

/** Localize retained structured notices on every render, including history replay. */
export function systemNoticeMessage(notice: AppProtocol.AgentSystemNotice): string {
  const content = notice.content;
  switch (content.type) {
    case "text": return content.data;
    case "error": return apiErrorMessage(content.data);
    case "promptWarning": {
      const { warning, message } = content.data;
      return i18n.t(`chat:promptWarning.${warning.type}`, {
        ...warning,
        ...("file" in warning ? { file: `${warning.file.toUpperCase()}.md` } : {}),
        defaultValue: message,
      });
    }
  }
}
