use super::*;

pub struct IdentityRequestProcessor {
    agent_name: String,
    agent_description: String,
}

impl IdentityRequestProcessor {
    pub fn new(agent_name: impl Into<String>, agent_description: impl Into<String>) -> Self {
        Self {
            agent_name: agent_name.into(),
            agent_description: agent_description.into(),
        }
    }
}

#[async_trait]
impl RequestProcessor for IdentityRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Identity
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if request.prompt_prefix_messages.is_empty() {
            let mut instruction = format!(
                "You are an agent. Your internal name is \"{}\".",
                self.agent_name
            );
            if !self.agent_description.trim().is_empty() {
                instruction.push_str(&format!(
                    " The description about you is \"{}\".",
                    self.agent_description
                ));
            }
            request
                .prompt_prefix_messages
                .push(ChatMessage::system(instruction));
        }
        Ok(RequestProcessorOutput::default())
    }
}
