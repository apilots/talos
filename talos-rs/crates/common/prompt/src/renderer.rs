use crate::PromptAssembly;
use talos_llm_provider::ChatMessage;
use talos_memory::TalosMessageRole;

pub(crate) fn talos_message_to_chat_message(role: TalosMessageRole, text: String) -> ChatMessage {
    match role {
        TalosMessageRole::User => ChatMessage::user(text),
        TalosMessageRole::Assistant => ChatMessage::assistant(text),
        TalosMessageRole::System => ChatMessage::system(text),
    }
}

pub(crate) fn render_assembly(assembly: &PromptAssembly) -> Vec<ChatMessage> {
    let mut messages =
        Vec::with_capacity(assembly.system_sections.len() + assembly.context_sections.len() + 1);
    messages.extend(
        assembly
            .system_sections
            .iter()
            .map(|section| match &section.role {
                Some(role) => talos_message_to_chat_message(role.clone(), section.content.clone()),
                None => ChatMessage::system(section.content.clone()),
            }),
    );
    messages.extend(
        assembly
            .context_sections
            .iter()
            .map(|section| match &section.role {
                Some(role) => talos_message_to_chat_message(role.clone(), section.content.clone()),
                None => ChatMessage::system(section.content.clone()),
            }),
    );
    messages.push(ChatMessage::user(assembly.user_input.clone()));
    messages
}
