use super::memory_summary::{
    estimate_transcript_tokens, render_transcript, trim_transcript_units_to_budget,
};
use crate::{SessionPromptContext, SessionPromptContextConfig, SessionPromptContextSource};
use talos_error::{ErrorCode, TalosError, TalosResult};

pub(super) struct MemoryContextBudget {
    pub tokens: usize,
    pub message_max_chars: Option<usize>,
    pub tool_results_keep: usize,
    pub tool_exchanges_keep: usize,
}

/// Reload a smaller canonical prefix when Turn grouping would discard all
/// readable history. The source, not a sliced projection, owns new coverage.
pub(super) async fn load_memory_context(
    source: &dyn SessionPromptContextSource,
    mut config: SessionPromptContextConfig,
    budget: Option<MemoryContextBudget>,
) -> TalosResult<SessionPromptContext> {
    loop {
        let mut context = source.load(config.clone()).await?;
        let Some(budget) = &budget else {
            return Ok(context);
        };
        if let Some(max_chars) = budget.message_max_chars {
            for message in &mut context.recent_messages {
                if message.text.chars().count() > max_chars {
                    message.text = message.text.chars().take(max_chars).collect();
                }
            }
        }
        let (units, _, _) = trim_transcript_units_to_budget(
            &context,
            budget.tokens,
            budget.tool_results_keep,
            budget.tool_exchanges_keep,
        );
        if budget.tokens == 0
            || units.iter().any(|unit| unit.is_history_turn())
            || context
                .recent_messages
                .iter()
                .all(|message| message.text.trim().is_empty())
        {
            return Ok(context);
        }
        let mut prefix = context.clone();
        prefix.recent_messages.clear();
        let mut fitting_messages = 0;
        for message in &context.recent_messages {
            prefix.recent_messages.push(message.clone());
            let (units, _, _) = trim_transcript_units_to_budget(
                &prefix,
                budget.tokens,
                budget.tool_results_keep,
                budget.tool_exchanges_keep,
            );
            if !units.iter().any(|unit| unit.is_history_turn())
                || estimate_transcript_tokens(&render_transcript(&units)) > budget.tokens
            {
                break;
            }
            fitting_messages += 1;
        }
        let next_limit = fitting_messages.max(1);
        if config
            .source_message_limit
            .is_some_and(|limit| next_limit >= limit)
        {
            return Err(TalosError::new(
                ErrorCode::ContextWindowExceeded,
                "One history message exceeds the memory transcript budget.",
            ));
        }
        config.source_message_limit = Some(next_limit);
    }
}
