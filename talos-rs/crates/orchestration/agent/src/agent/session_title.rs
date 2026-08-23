use talos_llm_provider::ChatMessage;
use talos_types::{deterministic_session_title, session_title_or_fallback, ModelUsageKind};
use tracing::warn;

use crate::{AgentRunSessionState, InnerModelChatParams, InnerModelClient};

const SESSION_TITLE_PROMPT: &str = r#"Generate a concise session title for the conversation.

Rules:
- Return only the title text.
- One line only.
- No markdown, no quotes, no trailing punctuation.
- Prefer 3-8 words.
- Focus on the core task topic.
"#;

/// Generates the first meaningful title for a session through the Inner Model.
///
/// Title generation is best-effort: an unavailable model falls back to a
/// deterministic title so the main Agent turn remains successful.
pub(super) async fn generate(
    inner_model: &InnerModelClient,
    session_state: &AgentRunSessionState,
    user_id: &str,
    session_id: &str,
    first_user_text: &str,
    first_assistant_text: &str,
) -> Option<String> {
    if session_state.has_real_title() || first_user_text.trim().is_empty() {
        return None;
    }

    let messages = vec![
        ChatMessage::system(SESSION_TITLE_PROMPT),
        ChatMessage::user(format!(
            "First user message:\n{}\n\nFirst assistant response:\n{}",
            first_user_text.trim(),
            first_assistant_text.trim()
        )),
    ];

    match inner_model
        .chat(InnerModelChatParams {
            user_id: user_id.to_string(),
            session_id: Some(session_id.to_string()),
            usage_kind: ModelUsageKind::Title,
            messages,
        })
        .await
    {
        Ok(response) => Some(session_title_or_fallback(response.content, first_user_text)),
        Err(error) => {
            warn!(?error, "failed to generate session title");
            Some(deterministic_session_title(first_user_text))
        }
    }
}
