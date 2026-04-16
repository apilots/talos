use crate::{CacheScope, PromptAssembly};
use talos_llm_provider::{CacheControl, CacheControlKind, ChatMessage};
use talos_memory::TalosMessageRole;

pub(crate) fn talos_message_to_chat_message(role: TalosMessageRole, text: String) -> ChatMessage {
    match role {
        TalosMessageRole::User => ChatMessage::user(text),
        TalosMessageRole::Assistant => ChatMessage::assistant(text),
        TalosMessageRole::System => ChatMessage::system(text),
    }
}

fn cache_control_from_scope(scope: CacheScope) -> Option<CacheControl> {
    match scope {
        CacheScope::Static | CacheScope::Workspace => Some(CacheControl {
            kind: CacheControlKind::Ephemeral,
        }),
        CacheScope::None => None,
    }
}

pub(crate) fn render_assembly(assembly: &PromptAssembly) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(
        assembly.static_prefix_sections.len() + assembly.dynamic_suffix_sections.len() + 1,
    );
    messages.extend(assembly.static_prefix_sections.iter().map(|section| {
        let mut message = match &section.role {
            Some(role) => talos_message_to_chat_message(role.clone(), section.content.clone()),
            None => ChatMessage::system(section.content.clone()),
        };
        message.cache_control = cache_control_from_scope(section.cache_scope);
        message
    }));
    messages.extend(assembly.dynamic_suffix_sections.iter().map(|section| {
        let mut message = match &section.role {
            Some(role) => talos_message_to_chat_message(role.clone(), section.content.clone()),
            None => ChatMessage::system(section.content.clone()),
        };
        message.cache_control = cache_control_from_scope(section.cache_scope);
        message
    }));
    messages.push(ChatMessage::user(assembly.user_input.clone()));
    messages
}
