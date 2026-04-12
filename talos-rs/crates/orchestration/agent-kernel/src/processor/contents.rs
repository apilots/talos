use super::*;

pub struct ContentsRequestProcessor;

#[async_trait]
impl RequestProcessor for ContentsRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Contents
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let mut messages = request.prompt_prefix_messages.clone();
        messages.extend(ContentsBuilder::build_messages(
            &ctx.session,
            &ctx.invocation_id,
            request.branch,
            request.contents_mode,
        ));
        messages.extend(request.transient_messages.clone());
        request.messages = messages;
        Ok(RequestProcessorOutput::default())
    }
}
