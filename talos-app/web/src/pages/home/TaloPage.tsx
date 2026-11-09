import { useTranslation } from "react-i18next";
import { TaloPanel, taloHasConversation, useManagedWorkerStatus } from "@/features/computers";
import { ChatSessionContent } from "@/pages/chat/ChatSessionPage";

export function TaloPage() {
  const { t } = useTranslation("computers");
  const instance = useManagedWorkerStatus().status.data?.instance;
  if (taloHasConversation(instance)) {
    return <ChatSessionContent key={instance.conversationSessionId} sessionId={instance.conversationSessionId} talo={instance} />;
  }
  return (
    <div className="h-full overflow-y-auto px-5 pb-8 pt-10 sm:px-6 sm:pt-16">
      <section className="mx-auto w-full max-w-[512px]" aria-labelledby="talo-page-title">
        <h1 id="talo-page-title" className="text-[24px] font-semibold tracking-tight text-ink">{t("talo.title")}</h1>
        <TaloPanel />
      </section>
    </div>
  );
}
